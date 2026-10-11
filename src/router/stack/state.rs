use super::NavigationError;
use std::{collections::BTreeMap, rc::Rc};

type Encode<C> = dyn Fn(&[Rc<C>]) -> Result<Vec<u8>, NavigationError>;
type Decode<C> = dyn Fn(&[u8]) -> Result<Vec<C>, NavigationError>;

type Policy<C> = dyn Fn(&[Rc<C>]) -> bool;
/// Explicit serialization of an entire configuration stack, without serde.
///
/// Child component state is framed separately by the stack router. Decode
/// errors are returned to the caller; restoration never silently resets.
pub struct StackCodec<C> {
    encode: Rc<Encode<C>>,
    decode: Rc<Decode<C>>,
    should_save: Option<Rc<Policy<C>>>,
    should_restore: Option<Rc<Policy<C>>>,
}
impl<C> StackCodec<C> {
    /// Chooses whether each save includes this router and its nested child states.
    /// Returning false skips the encoder and child state providers for that save.
    /// Defaults to true. Called synchronously on the current navigation snapshot.
    pub fn should_save(mut self, predicate: impl Fn(&[Rc<C>]) -> bool + 'static) -> Self {
        self.should_save = Some(Rc::new(predicate));
        self
    }
    /// Chooses whether to accept fully decoded, validated saved navigation.
    /// Returning false uses the lazy initializer and discards saved child state.
    /// Corrupt data and codec errors remain errors and never call this predicate.
    /// Defaults to true; an absent saved state never calls this predicate.
    pub fn should_restore(mut self, predicate: impl Fn(&[Rc<C>]) -> bool + 'static) -> Self {
        self.should_restore = Some(Rc::new(predicate));
        self
    }
    pub(super) fn can_save(&self, state: &[Rc<C>]) -> bool {
        self.should_save
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }
    pub(super) fn can_restore(&self, state: &[Rc<C>]) -> bool {
        self.should_restore
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }

    /// Supplies stack-wide configuration encoding and decoding callbacks.
    pub fn new(
        encode: impl Fn(&[Rc<C>]) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<Vec<C>, String> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(move |value| encode(value).map_err(NavigationError::state_message)),
            decode: Rc::new(move |bytes| decode(bytes).map_err(NavigationError::state_message)),
            should_save: None,
            should_restore: None,
        }
    }
    /// Uses typed navigation errors, including `NavigationError::state_source`.
    /// Callbacks are retained for future saves/restoration and must own captures.
    pub fn new_fallible(
        encode: impl Fn(&[Rc<C>]) -> Result<Vec<u8>, NavigationError> + 'static,
        decode: impl Fn(&[u8]) -> Result<Vec<C>, NavigationError> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(encode),
            decode: Rc::new(decode),
            should_save: None,
            should_restore: None,
        }
    }
}
impl<C> Clone for StackCodec<C> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
            should_save: self.should_save.clone(),
            should_restore: self.should_restore.clone(),
        }
    }
}
use crate::router::engine::SavedState as SavedStack;
const MAGIC: &[u8; 4] = b"DST1";
fn state_error(message: impl Into<String>) -> NavigationError {
    NavigationError::state_message(message.into())
}
fn append_field(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), NavigationError> {
    let length =
        u32::try_from(bytes.len()).map_err(|_| state_error("DST1 field exceeds u32 length"))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}
pub(crate) fn encode<C>(
    codec: &StackCodec<C>,
    configurations: &[Rc<C>],
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next_key: u64,
) -> Result<Vec<u8>, NavigationError> {
    if configurations.is_empty() {
        return Err(NavigationError::EmptyStack);
    }
    if children.len() != configurations.len() {
        return Err(state_error(
            "DST1 child count does not match configurations",
        ));
    }
    let count =
        u32::try_from(children.len()).map_err(|_| state_error("DST1 child count exceeds u32"))?;
    let mut keys = std::collections::BTreeSet::new();
    for (key, _) in children {
        if !keys.insert(key) {
            return Err(state_error("duplicate child key in DST1"));
        }
    }
    let payload = (codec.encode)(configurations)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&next_key.to_le_bytes());
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
            .ok_or_else(|| state_error("truncated DST1 envelope"))?;
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
    codec: &StackCodec<C>,
    bytes: &[u8],
) -> Result<SavedStack<Vec<Rc<C>>>, NavigationError> {
    let mut reader = Reader { bytes, cursor: 0 };
    if reader.take(4)? != MAGIC {
        return Err(state_error("bad DST1 magic"));
    }
    let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
    let payload = reader.field()?;
    let count = reader.u32()? as usize;
    // Every child requires two lengths and at least an eight-byte DCS1 frame.
    if count > (bytes.len() - reader.cursor) / 16 {
        return Err(state_error("truncated DST1 children"));
    }
    let mut children = Vec::with_capacity(count);
    let mut keys = std::collections::BTreeSet::new();
    for _ in 0..count {
        let key = std::str::from_utf8(reader.field()?)
            .map_err(|_| state_error("invalid UTF-8 in DST1 child key"))?
            .to_owned();
        if !keys.insert(key.clone()) {
            return Err(state_error("duplicate child key in DST1"));
        }
        let state = crate::framing::decode(reader.field()?)
            .map_err(|error| state_error(error.to_string()))?;
        children.push((key, state));
    }
    if reader.cursor != bytes.len() {
        return Err(state_error("trailing bytes after DST1 envelope"));
    }
    let configurations = (codec.decode)(payload)?;
    if configurations.is_empty() {
        return Err(NavigationError::EmptyStack);
    }
    if configurations.len() != children.len() {
        return Err(state_error(
            "DST1 child count does not match configurations",
        ));
    }
    Ok(SavedStack {
        configurations: configurations.into_iter().map(Rc::new).collect(),
        children,
        next_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn codec() -> StackCodec<u8> {
        StackCodec::new(
            |configs| Ok(configs.iter().map(|c| **c).collect()),
            |bytes| Ok(bytes.to_vec()),
        )
    }
    fn sample() -> Vec<u8> {
        encode(
            &codec(),
            &[Rc::new(1), Rc::new(2)],
            &[
                (
                    "stack/0".into(),
                    BTreeMap::from([("nested".into(), vec![0, 255])]),
                ),
                ("stack/1".into(), BTreeMap::new()),
            ],
            2,
        )
        .unwrap()
    }
    #[test]
    fn dst1_bytes_match_the_pre_slot_fixture() {
        let fixture: &[u8] = &[
            68, 83, 84, 49, 2, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 2, 2, 0, 0, 0, 7, 0, 0, 0, 115,
            116, 97, 99, 107, 47, 48, 24, 0, 0, 0, 68, 67, 83, 49, 1, 0, 0, 0, 6, 0, 0, 0, 110,
            101, 115, 116, 101, 100, 2, 0, 0, 0, 0, 255, 7, 0, 0, 0, 115, 116, 97, 99, 107, 47, 49,
            8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0, 0,
        ];
        assert_eq!(sample(), fixture);
        assert_eq!(decode(&codec(), fixture).unwrap().next_key, 2);
    }
    #[test]
    fn round_trip_keeps_configuration_child_order_and_counter() {
        let restored = decode(&codec(), &sample()).unwrap();
        assert_eq!(
            restored
                .configurations
                .iter()
                .map(|c| **c)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(
            restored.children[0],
            (
                "stack/0".into(),
                BTreeMap::from([("nested".into(), vec![0, 255])])
            )
        );
        assert_eq!(restored.children[1], ("stack/1".into(), BTreeMap::new()));
        assert_eq!(restored.next_key, 2);
        assert_eq!(sample(), sample());
    }
    #[test]
    fn all_truncated_prefixes_and_trailing_bytes_are_rejected() {
        let bytes = sample();
        for len in 0..bytes.len() {
            assert!(decode(&codec(), &bytes[..len]).is_err(), "prefix {len}");
        }
        let mut bytes = bytes;
        bytes.push(0);
        assert!(decode(&codec(), &bytes).is_err());
    }
    #[test]
    fn empty_stack_count_mismatch_and_duplicate_keys_are_rejected() {
        assert!(encode(&codec(), &[], &[], 0).is_err());
        assert!(encode(&codec(), &[Rc::new(1)], &[], 1).is_err());
        assert!(
            encode(
                &codec(),
                &[Rc::new(1), Rc::new(2)],
                &[
                    ("stack/0".into(), BTreeMap::new()),
                    ("stack/0".into(), BTreeMap::new())
                ],
                1
            )
            .is_err()
        );
        let mismatched = StackCodec::new(|_: &[Rc<u8>]| Ok(vec![]), |_| Ok(vec![1u8]));
        assert!(decode(&mismatched, &sample()).is_err());
        let empty = StackCodec::new(|_: &[Rc<u8>]| Ok(vec![]), |_| Ok(Vec::<u8>::new()));
        assert!(decode(&empty, &sample()).is_err());
    }
    #[test]
    fn corrupt_lengths_magic_utf8_and_nested_frames_are_rejected() {
        let mut bytes = sample();
        bytes[0] = 0;
        assert!(decode(&codec(), &bytes).is_err());
        let mut bytes = sample();
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&codec(), &bytes).is_err());
        let mut bytes = sample();
        bytes[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&codec(), &bytes).is_err());
        let mut bytes = sample();
        bytes[26] = 255;
        assert!(decode(&codec(), &bytes).is_err());
        let mut bytes = sample();
        bytes[37] = 0;
        assert!(decode(&codec(), &bytes).is_err());
    }
    #[test]
    fn encoded_duplicate_keys_and_user_codec_failures_are_rejected() {
        let mut bytes = sample();
        let last_key = bytes
            .windows(7)
            .rposition(|window| window == b"stack/1")
            .unwrap();
        bytes[last_key..last_key + 7].copy_from_slice(b"stack/0");
        assert!(
            matches!(decode(&codec(), &bytes), Err(NavigationError::StateSource(message)) if message.to_string().contains("duplicate"))
        );
        let encode_failure = StackCodec::new(
            |_: &[Rc<u8>]| Err("encode failure".into()),
            |_| Ok(vec![1u8]),
        );
        assert!(
            matches!(encode(&encode_failure, &[Rc::new(1)], &[("stack/0".into(), BTreeMap::new())], 1), Err(NavigationError::StateSource(message)) if message.to_string() == "encode failure")
        );
        let decode_failure = StackCodec::new(
            |_: &[Rc<u8>]| Ok(vec![]),
            |_| Err::<Vec<u8>, _>("decode failure".into()),
        );
        assert!(
            matches!(decode(&decode_failure, &sample()), Err(NavigationError::StateSource(message)) if message.to_string() == "decode failure")
        );
    }
}
