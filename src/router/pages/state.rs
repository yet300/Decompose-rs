use super::{NavigationError, Pages};
use std::{collections::BTreeMap, rc::Rc};

type Encode<C> = dyn Fn(&[Rc<C>]) -> Result<Vec<u8>, String>;
type Decode<C> = dyn Fn(&[u8]) -> Result<Vec<C>, String>;

/// Explicit serialization of an ordered page configuration list, without serde.
///
/// Child component state is framed separately by the pages router. Decode
/// errors are returned to the caller; restoration never silently resets.
pub struct PagesCodec<C> {
    encode: Rc<Encode<C>>,
    decode: Rc<Decode<C>>,
}
impl<C> PagesCodec<C> {
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
impl<C> Clone for PagesCodec<C> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
        }
    }
}
use crate::router::engine::SavedState as SavedPages;
const MAGIC: &[u8; 4] = b"DPG1";
fn state_error(message: impl Into<String>) -> NavigationError {
    NavigationError::State(message.into())
}
fn append_field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), NavigationError> {
    let length =
        u32::try_from(bytes.len()).map_err(|_| state_error("DPG1 field exceeds u32 length"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
pub(crate) fn encode<C>(
    codec: &PagesCodec<C>,
    pages: &Pages<C>,
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next_key: u64,
) -> Result<Vec<u8>, NavigationError> {
    pages.validate()?;
    let configurations = &pages.items;
    if children.len() != configurations.len() {
        return Err(state_error(
            "DPG1 child count does not match configurations",
        ));
    }
    let count =
        u32::try_from(children.len()).map_err(|_| state_error("DPG1 child count exceeds u32"))?;
    let mut keys = std::collections::BTreeSet::new();
    for (key, _) in children {
        validate_key(key, next_key)?;
        if !keys.insert(key) {
            return Err(state_error("duplicate child key in DPG1"));
        }
    }
    let payload = (codec.encode)(configurations).map_err(NavigationError::State)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&next_key.to_le_bytes());
    let selection = pages
        .selected_index
        .map(|i| u64::try_from(i).map_err(|_| state_error("DPG1 selection exceeds u64")))
        .transpose()?
        .unwrap_or(u64::MAX);
    out.extend_from_slice(&selection.to_le_bytes());
    append_field(&mut out, &payload)?;
    out.extend_from_slice(&count.to_le_bytes());
    for (key, state) in children {
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
            .ok_or_else(|| state_error("truncated DPG1 envelope"))?;
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
pub(crate) fn decode<C>(
    codec: &PagesCodec<C>,
    bytes: &[u8],
) -> Result<SavedPages<Pages<C>>, NavigationError> {
    let mut reader = Reader { bytes, cursor: 0 };
    if reader.take(4)? != MAGIC {
        return Err(state_error("bad DPG1 magic"));
    }
    let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let selection = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let payload = reader.field()?;
    let count = reader.u32()? as usize;
    let selected_index = if selection == u64::MAX {
        None
    } else {
        Some(usize::try_from(selection).map_err(|_| state_error("DPG1 selection exceeds usize"))?)
    };
    if (count == 0 && selected_index.is_some())
        || (count > 0 && selected_index.is_none_or(|i| i >= count))
    {
        return Err(NavigationError::InvalidSelection {
            selected_index,
            item_count: count,
        });
    }
    // Every child requires two lengths and at least an eight-byte DCS1 frame.
    if count > (bytes.len() - reader.cursor) / 16 {
        return Err(state_error("truncated DPG1 children"));
    }
    let mut children = Vec::with_capacity(count);
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..count {
        let key = std::str::from_utf8(reader.field()?)
            .map_err(|_| state_error("invalid UTF-8 in DPG1 child key"))?
            .to_owned();
        validate_key(&key, next_key)?;
        if !keys.insert(key.clone()) {
            return Err(state_error("duplicate child key in DPG1"));
        }
        let state = crate::framing::decode(reader.field()?)
            .map_err(|error| state_error(error.to_string()))?;
        children.push((key, state));
    }
    if reader.cursor != bytes.len() {
        return Err(state_error("trailing bytes after DPG1 envelope"));
    }
    let configurations = (codec.decode)(payload).map_err(NavigationError::State)?;
    if configurations.len() != children.len() {
        return Err(state_error(
            "DPG1 child count does not match configurations",
        ));
    }
    Ok(SavedPages {
        configurations: Pages {
            items: configurations.into_iter().map(Rc::new).collect(),
            selected_index,
        },
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
