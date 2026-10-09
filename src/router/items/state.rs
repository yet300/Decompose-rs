use super::{ActiveLifecycleState, ItemMap, Items, NavigationError};
use std::{collections::BTreeMap, rc::Rc};

type Encode<C> = dyn Fn(&[Rc<C>]) -> Result<Vec<u8>, String>;
type Decode<C> = dyn Fn(&[u8]) -> Result<Vec<C>, String>;

/// Explicit serialization of an ordered item configuration list, without serde.
///
/// Child component state is framed separately by the items router. Decode
/// errors are returned to the caller; restoration never silently resets.
pub struct ItemsCodec<C> {
    encode: Rc<Encode<C>>,
    decode: Rc<Decode<C>>,
}
impl<C> ItemsCodec<C> {
    /// Supplies list-wide configuration encoding and decoding callbacks.
    pub fn new(
        encode: impl Fn(&[Rc<C>]) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<Vec<C>, String> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(encode),
            decode: Rc::new(decode),
        }
    }
}
impl<C> Clone for ItemsCodec<C> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
        }
    }
}
use crate::router::engine::SavedState as SavedItems;
const MAGIC: &[u8; 4] = b"DPI1";
fn state_error(message: impl Into<String>) -> NavigationError {
    NavigationError::State(message.into())
}
fn append_field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), NavigationError> {
    let length =
        u32::try_from(bytes.len()).map_err(|_| state_error("DPI1 field exceeds u32 length"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
pub(crate) fn encode<C: Eq>(
    codec: &ItemsCodec<C>,
    items: &Items<C>,
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next_key: u64,
) -> Result<Vec<u8>, NavigationError> {
    items.validate()?;
    let configurations = &items.items;
    if children.len() != configurations.len() {
        return Err(state_error(
            "DPI1 child count does not match configurations",
        ));
    }
    let count =
        u32::try_from(children.len()).map_err(|_| state_error("DPI1 child count exceeds u32"))?;
    let mut keys = std::collections::BTreeSet::new();
    for (key, _) in children {
        validate_key(key, next_key)?;
        if !keys.insert(key) {
            return Err(state_error("duplicate child key in DPI1"));
        }
    }
    let payload = (codec.encode)(configurations).map_err(NavigationError::State)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&next_key.to_le_bytes());
    append_field(&mut out, &payload)?;
    out.extend_from_slice(&count.to_le_bytes());
    for ((key, state), configuration) in children.iter().zip(configurations) {
        out.push(match items.active_items.get(configuration) {
            None => 0,
            Some(ActiveLifecycleState::Created) => 1,
            Some(ActiveLifecycleState::Started) => 2,
            Some(ActiveLifecycleState::Resumed) => 3,
        });
        append_field(&mut out, key.as_bytes())?;
        // DCS1 uses u32 lengths too; check before its infallible encoder.
        u32::try_from(state.len()).map_err(|_| state_error("DCS1 entry count exceeds u32"))?;
        for (key, value) in state {
            u32::try_from(key.len()).map_err(|_| state_error("DCS1 key length exceeds u32"))?;
            u32::try_from(value.len()).map_err(|_| state_error("DCS1 value length exceeds u32"))?;
        }
        append_field(&mut out, &crate::framing::encode(state))?;
    }
    Ok(out)
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], NavigationError> {
        let end = self
            .cursor
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| state_error("truncated DPI1 envelope"))?;
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
pub(crate) fn decode<C: Eq>(
    codec: &ItemsCodec<C>,
    bytes: &[u8],
) -> Result<SavedItems<Items<C>>, NavigationError> {
    let mut reader = Reader { bytes, cursor: 0 };
    if reader.take(4)? != MAGIC {
        return Err(state_error("bad DPI1 magic"));
    }
    let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let payload = reader.field()?;
    let count = reader.u32()? as usize;
    // Every child requires a status byte, two lengths, and at least an eight-byte DCS1 frame.
    if count > (bytes.len() - reader.cursor) / 17 {
        return Err(state_error("truncated DPI1 children"));
    }
    let mut children = Vec::with_capacity(count);
    let mut targets = Vec::with_capacity(count);
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..count {
        targets.push(match reader.take(1)?[0] {
            0 => None,
            1 => Some(ActiveLifecycleState::Created),
            2 => Some(ActiveLifecycleState::Started),
            3 => Some(ActiveLifecycleState::Resumed),
            _ => return Err(state_error("invalid DPI1 child status")),
        });
        let key = std::str::from_utf8(reader.field()?)
            .map_err(|_| state_error("invalid UTF-8 in DPI1 child key"))?
            .to_owned();
        validate_key(&key, next_key)?;
        if !keys.insert(key.clone()) {
            return Err(state_error("duplicate child key in DPI1"));
        }
        let state = crate::framing::decode(reader.field()?)
            .map_err(|error| state_error(error.to_string()))?;
        children.push((key, state));
    }
    if reader.cursor != bytes.len() {
        return Err(state_error("trailing bytes after DPI1 envelope"));
    }
    let configurations = (codec.decode)(payload).map_err(NavigationError::State)?;
    if configurations.len() != children.len() {
        return Err(state_error(
            "DPI1 child count does not match configurations",
        ));
    }
    let mut configurations = Items {
        items: configurations.into_iter().map(Rc::new).collect(),
        active_items: ItemMap::new(),
    };
    configurations.validate()?;
    configurations.active_items = configurations
        .items
        .iter()
        .zip(targets)
        .filter_map(|(c, s)| s.map(|s| (c.clone(), s)))
        .collect();
    Ok(SavedItems {
        configurations,
        children,
        next_key,
    })
}

fn validate_key(key: &str, next_key: u64) -> Result<(), NavigationError> {
    let counter = key
        .strip_prefix("child/")
        .and_then(|s| s.parse::<u64>().ok());
    if !counter.is_some_and(|n| n < next_key && key == format!("child/{n}")) {
        return Err(state_error("invalid child identity or next-key counter"));
    }
    Ok(())
}
