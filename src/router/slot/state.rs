use super::NavigationError;
use crate::router::engine::SavedState;
use std::{collections::BTreeMap, rc::Rc};
type Encode<C> = dyn Fn(&C) -> Result<Vec<u8>, String>;
type Decode<C> = dyn Fn(&[u8]) -> Result<C, String>;
type Policy<C> = dyn Fn(Option<&C>) -> bool;
/// Explicit serialization of a populated slot's configuration, without serde.
/// Empty slots never invoke configuration encoding or decoding; membership and child
/// state are framed separately. Persistence predicates still receive `None`.
pub struct SlotCodec<C> {
    encode: Rc<Encode<C>>,
    decode: Rc<Decode<C>>,
    should_save: Option<Rc<Policy<C>>>,
    should_restore: Option<Rc<Policy<C>>>,
}
impl<C> Clone for SlotCodec<C> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
            should_save: self.should_save.clone(),
            should_restore: self.should_restore.clone(),
        }
    }
}
impl<C> SlotCodec<C> {
    /// Chooses whether each save includes this router and its nested child states.
    /// Returning false skips the encoder and child state providers for that save.
    /// Defaults to true. Called synchronously on the current navigation snapshot.
    pub fn should_save(mut self, predicate: impl Fn(Option<&C>) -> bool + 'static) -> Self {
        self.should_save = Some(Rc::new(predicate));
        self
    }
    /// Chooses whether to accept fully decoded, validated saved navigation.
    /// Returning false uses the lazy initializer and discards saved child state.
    /// Corrupt data and codec errors remain errors and never call this predicate.
    /// Defaults to true; an absent saved state never calls this predicate.
    pub fn should_restore(mut self, predicate: impl Fn(Option<&C>) -> bool + 'static) -> Self {
        self.should_restore = Some(Rc::new(predicate));
        self
    }
    pub(super) fn can_save(&self, state: Option<&C>) -> bool {
        self.should_save
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }
    pub(super) fn can_restore(&self, state: Option<&C>) -> bool {
        self.should_restore
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }

    /// Supplies configuration encoding and decoding callbacks.
    pub fn new(
        encode: impl Fn(&C) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<C, String> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(encode),
            decode: Rc::new(decode),
            should_save: None,
            should_restore: None,
        }
    }
}

fn error(message: impl Into<String>) -> NavigationError {
    NavigationError::State(message.into())
}
fn field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), NavigationError> {
    let length = u32::try_from(bytes.len()).map_err(|_| error("DSL1 field exceeds u32 length"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
pub(super) fn encode<C>(
    codec: &SlotCodec<C>,
    configs: &[Rc<C>],
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next_key: u64,
) -> Result<Vec<u8>, NavigationError> {
    if configs.len() > 1 || configs.len() != children.len() {
        return Err(error("invalid DSL1 membership count"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"DSL1");
    out.extend_from_slice(&next_key.to_le_bytes());
    out.extend_from_slice(&(configs.len() as u32).to_le_bytes());
    if let Some(config) = configs.first() {
        let (key, state) = &children[0];
        validate_key(key, next_key)?;
        let payload = (codec.encode)(config).map_err(NavigationError::State)?;
        field(&mut out, &payload)?;
        field(&mut out, key.as_bytes())?;
        u32::try_from(state.len()).map_err(|_| error("DCS1 entry count exceeds u32"))?;
        for (key, value) in state {
            u32::try_from(key.len()).map_err(|_| error("DCS1 key length exceeds u32"))?;
            u32::try_from(value.len()).map_err(|_| error("DCS1 value length exceeds u32"))?;
        }
        field(&mut out, &crate::framing::encode(state))?;
    }
    Ok(out)
}
fn validate_key(key: &str, next_key: u64) -> Result<(), NavigationError> {
    let counter = key
        .strip_prefix("child/")
        .and_then(|s| s.parse::<u64>().ok());
    if !counter.is_some_and(|n| n < next_key && key == format!("child/{n}")) {
        return Err(error("invalid child identity or next-key counter"));
    }
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], NavigationError> {
        let end = self
            .cursor
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| error("truncated DSL1 envelope"))?;
        let bytes = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(bytes)
    }
    fn u32(&mut self) -> Result<u32, NavigationError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn field(&mut self) -> Result<&'a [u8], NavigationError> {
        let length = self.u32()? as usize;
        self.take(length)
    }
}
pub(super) fn decode<C>(
    codec: &SlotCodec<C>,
    bytes: &[u8],
) -> Result<SavedState<Vec<Rc<C>>>, NavigationError> {
    let mut reader = Reader { bytes, cursor: 0 };
    if reader.take(4)? != b"DSL1" {
        return Err(error("bad DSL1 magic"));
    }
    let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let count = reader.u32()?;
    if count > 1 {
        return Err(error("invalid DSL1 membership count"));
    }
    let child = if count == 1 {
        let payload = reader.field()?;
        let key = std::str::from_utf8(reader.field()?)
            .map_err(|_| error("invalid UTF-8 in DSL1 child key"))?
            .to_owned();
        validate_key(&key, next_key)?;
        let state = crate::framing::decode(reader.field()?).map_err(|e| error(e.to_string()))?;
        Some((payload, key, state))
    } else {
        None
    };
    if reader.cursor != bytes.len() {
        return Err(error("trailing bytes after DSL1 envelope"));
    }
    let (configurations, children) = match child {
        Some((payload, key, state)) => (
            vec![Rc::new(
                (codec.decode)(payload).map_err(NavigationError::State)?,
            )],
            vec![(key, state)],
        ),
        None => (Vec::new(), Vec::new()),
    };
    Ok(SavedState {
        configurations,
        children,
        next_key,
    })
}
