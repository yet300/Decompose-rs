use super::{NavState, NavigationError};
use std::{collections::BTreeMap, rc::Rc};
type Encode<N> = dyn Fn(&N) -> Result<Option<Vec<u8>>, NavigationError>;
type Decode<N> = dyn Fn(&[u8]) -> Result<Option<N>, NavigationError>;
/// Optional whole-navigation-state serialization, without a serde requirement.
///
/// Encoding `None` omits persistence. Decoding `None` explicitly declines a
/// structurally valid frame and invokes lazy initialization instead. Errors
/// propagate. Codecs must preserve child configuration identity and order.
pub struct NavStateCodec<N> {
    encode: Rc<Encode<N>>,
    decode: Rc<Decode<N>>,
}
impl<N> NavStateCodec<N> {
    /// Supplies optional encoding and decoding callbacks.
    pub fn new(
        encode: impl Fn(&N) -> Result<Option<Vec<u8>>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<Option<N>, String> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(move |value| encode(value).map_err(NavigationError::State)),
            decode: Rc::new(move |bytes| decode(bytes).map_err(NavigationError::State)),
        }
    }
    /// Uses typed navigation errors, including `NavigationError::state_source`.
    /// Callbacks are retained for future saves/restoration and must own captures.
    pub fn new_fallible(
        encode: impl Fn(&N) -> Result<Option<Vec<u8>>, NavigationError> + 'static,
        decode: impl Fn(&[u8]) -> Result<Option<N>, NavigationError> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(encode),
            decode: Rc::new(decode),
        }
    }
}
impl<N> Clone for NavStateCodec<N> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
        }
    }
}

impl<N> NavStateCodec<N> {
    pub(crate) fn encode_payload(&self, state: &N) -> Result<Option<Vec<u8>>, NavigationError> {
        (self.encode)(state)
    }
}
fn error(message: impl Into<String>) -> NavigationError {
    NavigationError::State(message.into())
}
fn field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), NavigationError> {
    let length = u32::try_from(bytes.len()).map_err(|_| error("DCH1 field exceeds u32 length"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
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
pub(crate) fn encode_payload(
    payload: &[u8],
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next_key: u64,
) -> Result<Vec<u8>, NavigationError> {
    let count = u32::try_from(children.len()).map_err(|_| error("DCH1 child count exceeds u32"))?;
    let mut out = Vec::new();
    out.extend_from_slice(b"DCH1");
    out.extend_from_slice(&next_key.to_le_bytes());
    field(&mut out, payload)?;
    out.extend_from_slice(&count.to_le_bytes());
    let mut keys = std::collections::BTreeSet::new();
    for (key, state) in children {
        validate_key(key, next_key)?;
        if !keys.insert(key) {
            return Err(error("duplicate child key in DCH1"));
        }
        field(&mut out, key.as_bytes())?;
        u32::try_from(state.len()).map_err(|_| error("DCS1 entry count exceeds u32"))?;
        for (key, bytes) in state {
            u32::try_from(key.len()).map_err(|_| error("DCS1 key length exceeds u32"))?;
            u32::try_from(bytes.len()).map_err(|_| error("DCS1 value length exceeds u32"))?;
        }
        field(&mut out, &crate::framing::encode(state))?;
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
            .ok_or_else(|| error("truncated DCH1 envelope"))?;
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
pub(crate) fn decode<C, N: NavState<C>>(
    codec: &NavStateCodec<N>,
    bytes: &[u8],
) -> Result<Option<crate::router::engine::SavedState<N>>, NavigationError> {
    let mut reader = Reader { bytes, cursor: 0 };
    if reader.take(4)? != b"DCH1" {
        return Err(error("bad DCH1 magic"));
    }
    let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let payload = reader.field()?;
    let count = reader.u32()? as usize;
    if count > (bytes.len() - reader.cursor) / 16 {
        return Err(error("truncated DCH1 children"));
    }
    let mut children = Vec::with_capacity(count);
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..count {
        let key = std::str::from_utf8(reader.field()?)
            .map_err(|_| error("invalid UTF-8 in DCH1 child key"))?
            .to_owned();
        validate_key(&key, next_key)?;
        if !keys.insert(key.clone()) {
            return Err(error("duplicate child key in DCH1"));
        }
        let state = crate::framing::decode(reader.field()?).map_err(|e| error(e.to_string()))?;
        children.push((key, state));
    }
    if reader.cursor != bytes.len() {
        return Err(error("trailing bytes after DCH1 envelope"));
    }
    let Some(configurations) = (codec.decode)(payload)? else {
        return Ok(None);
    };
    if configurations.children().len() != count {
        return Err(error("DCH1 child count does not match navigation state"));
    }
    Ok(Some(crate::router::engine::SavedState {
        configurations,
        children,
        next_key,
    }))
}
