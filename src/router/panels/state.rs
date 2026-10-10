use super::{ChildPanelsMode, NavigationError, Panels};
use crate::router::engine::SavedState;
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::Infallible,
    rc::Rc,
};

type Encode<MC, DC, EC> = dyn Fn(&MC, Option<&DC>, Option<&EC>) -> Result<Vec<u8>, NavigationError>;
type Decode<MC, DC, EC> = dyn Fn(&[u8]) -> Result<(MC, Option<DC>, Option<EC>), NavigationError>;
type EncodeState<MC, DC, EC> = dyn Fn(&Panels<MC, DC, EC>) -> Result<Vec<u8>, NavigationError>;
type DecodeState<MC, DC, EC> = dyn Fn(&[u8]) -> Result<Panels<MC, DC, EC>, NavigationError>;
enum Callbacks<MC, DC, EC> {
    Configurations {
        encode: Rc<Encode<MC, DC, EC>>,
        decode: Rc<Decode<MC, DC, EC>>,
    },
    State {
        encode: Rc<EncodeState<MC, DC, EC>>,
        decode: Rc<DecodeState<MC, DC, EC>>,
    },
}
impl<MC, DC, EC> Clone for Callbacks<MC, DC, EC> {
    fn clone(&self) -> Self {
        match self {
            Self::Configurations { encode, decode } => Self::Configurations {
                encode: encode.clone(),
                decode: decode.clone(),
            },
            Self::State { encode, decode } => Self::State {
                encode: encode.clone(),
                decode: decode.clone(),
            },
        }
    }
}
type Policy<MC, DC, EC> = dyn Fn(&Panels<MC, DC, EC>) -> bool;
/// Explicit panel serialization, independent of child state. [`Self::new`]
/// handles membership; [`Self::new_state`] also handles lifecycle mode.
/// The decoder must return the same optional-role presence encoded in the envelope.
pub struct PanelsCodec<MC, DC, EC = Infallible> {
    callbacks: Callbacks<MC, DC, EC>,
    should_save: Option<Rc<Policy<MC, DC, EC>>>,
    should_restore: Option<Rc<Policy<MC, DC, EC>>>,
}
impl<MC, DC, EC> PanelsCodec<MC, DC, EC> {
    /// Chooses whether each save includes this router and its nested child states.
    /// Returning false skips the encoder and child state providers for that save.
    /// Defaults to true. Called synchronously on the current navigation snapshot.
    pub fn should_save(
        mut self,
        predicate: impl Fn(&Panels<MC, DC, EC>) -> bool + 'static,
    ) -> Self {
        self.should_save = Some(Rc::new(predicate));
        self
    }
    /// Chooses whether to accept fully decoded, validated saved navigation.
    /// Returning false uses the lazy initializer and discards saved child state.
    /// Corrupt data and codec errors remain errors and never call this predicate.
    /// Defaults to true; an absent saved state never calls this predicate.
    pub fn should_restore(
        mut self,
        predicate: impl Fn(&Panels<MC, DC, EC>) -> bool + 'static,
    ) -> Self {
        self.should_restore = Some(Rc::new(predicate));
        self
    }
    pub(super) fn can_save(&self, state: &Panels<MC, DC, EC>) -> bool {
        self.should_save
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }
    pub(super) fn can_restore(&self, state: &Panels<MC, DC, EC>) -> bool {
        self.should_restore
            .as_ref()
            .is_none_or(|predicate| predicate(state))
    }

    /// Supplies whole-membership encoding and decoding callbacks.
    pub fn new(
        encode: impl Fn(&MC, Option<&DC>, Option<&EC>) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<(MC, Option<DC>, Option<EC>), String> + 'static,
    ) -> Self {
        Self {
            callbacks: Callbacks::Configurations {
                encode: Rc::new(move |main, details, extra| {
                    encode(main, details, extra).map_err(NavigationError::State)
                }),
                decode: Rc::new(move |bytes| decode(bytes).map_err(NavigationError::State)),
            },
            should_save: None,
            should_restore: None,
        }
    }

    /// Supplies whole-state callbacks, including panel lifecycle mode.
    /// The decoded state may override mode but must preserve the saved
    /// optional-role presence. Child states remain associated with their role.
    /// The library validates the envelope before decoding and role presence
    /// before child construction. Save/restore predicates remain available.
    pub fn new_state(
        encode: impl Fn(&Panels<MC, DC, EC>) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<Panels<MC, DC, EC>, String> + 'static,
    ) -> Self {
        Self {
            callbacks: Callbacks::State {
                encode: Rc::new(move |value| encode(value).map_err(NavigationError::State)),
                decode: Rc::new(move |bytes| decode(bytes).map_err(NavigationError::State)),
            },
            should_save: None,
            should_restore: None,
        }
    }
    /// Uses typed navigation errors, including `NavigationError::state_source`.
    /// Callbacks are retained for future saves/restoration and must own captures.
    pub fn new_fallible(
        encode: impl Fn(&MC, Option<&DC>, Option<&EC>) -> Result<Vec<u8>, NavigationError> + 'static,
        decode: impl Fn(&[u8]) -> Result<(MC, Option<DC>, Option<EC>), NavigationError> + 'static,
    ) -> Self {
        Self {
            callbacks: Callbacks::Configurations {
                encode: Rc::new(encode),
                decode: Rc::new(decode),
            },
            should_save: None,
            should_restore: None,
        }
    }
    /// Uses typed navigation errors, including `NavigationError::state_source`.
    /// Callbacks are retained for future saves/restoration and must own captures.
    pub fn new_state_fallible(
        encode: impl Fn(&Panels<MC, DC, EC>) -> Result<Vec<u8>, NavigationError> + 'static,
        decode: impl Fn(&[u8]) -> Result<Panels<MC, DC, EC>, NavigationError> + 'static,
    ) -> Self {
        Self {
            callbacks: Callbacks::State {
                encode: Rc::new(encode),
                decode: Rc::new(decode),
            },
            should_save: None,
            should_restore: None,
        }
    }
}
impl<MC, DC> PanelsCodec<MC, DC, Infallible> {
    /// Main/Details callbacks with typed errors and no Extra type argument.
    /// Stored callbacks must own their captures for future save/restore calls.
    pub fn without_extra_fallible(
        encode: impl Fn(&MC, Option<&DC>) -> Result<Vec<u8>, NavigationError> + 'static,
        decode: impl Fn(&[u8]) -> Result<(MC, Option<DC>), NavigationError> + 'static,
    ) -> Self {
        Self::new_fallible(
            move |main, details, extra| {
                if let Some(never) = extra {
                    match *never {}
                }
                encode(main, details)
            },
            move |bytes| decode(bytes).map(|(main, details)| (main, details, None)),
        )
    }
    /// Adapts Main/Details callbacks with Extra uninhabited by type.
    pub fn without_extra(
        encode: impl Fn(&MC, Option<&DC>) -> Result<Vec<u8>, String> + 'static,
        decode: impl Fn(&[u8]) -> Result<(MC, Option<DC>), String> + 'static,
    ) -> Self {
        Self::new(
            move |m, d, e| {
                if let Some(never) = e {
                    match *never {}
                }
                encode(m, d)
            },
            move |b| decode(b).map(|(m, d)| (m, d, None)),
        )
    }
}
impl<MC, DC, EC> Clone for PanelsCodec<MC, DC, EC> {
    fn clone(&self) -> Self {
        Self {
            callbacks: self.callbacks.clone(),
            should_save: self.should_save.clone(),
            should_restore: self.should_restore.clone(),
        }
    }
}
const MAGIC: &[u8; 4] = b"DPN1";
fn error(s: impl Into<String>) -> NavigationError {
    NavigationError::State(s.into())
}
fn field(out: &mut Vec<u8>, b: &[u8]) -> Result<(), NavigationError> {
    let n = u32::try_from(b.len()).map_err(|_| error("DPN1 field exceeds u32 length"))?;
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(b);
    Ok(())
}
fn validate_key(key: &str, next: u64) -> Result<(), NavigationError> {
    if !key
        .strip_prefix("child/")
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n < next && key == format!("child/{n}"))
    {
        return Err(error("invalid DPN1 child identity or next-key counter"));
    }
    Ok(())
}
pub(super) fn encode<MC, DC, EC>(
    codec: &PanelsCodec<MC, DC, EC>,
    p: &Panels<MC, DC, EC>,
    children: &[(String, BTreeMap<String, Vec<u8>>)],
    next: u64,
) -> Result<Vec<u8>, NavigationError> {
    let flags = u8::from(p.details.is_some()) | (u8::from(p.extra.is_some()) << 1);
    if children.len() != 1 + usize::from(p.details.is_some()) + usize::from(p.extra.is_some()) {
        return Err(error("DPN1 child count does not match membership"));
    }
    let mut keys = BTreeSet::new();
    for (key, state) in children {
        validate_key(key, next)?;
        if !keys.insert(key) {
            return Err(error("duplicate DPN1 child key"));
        }
        u32::try_from(state.len()).map_err(|_| error("DCS1 entry count exceeds u32"))?;
        for (k, v) in state {
            u32::try_from(k.len()).map_err(|_| error("DCS1 key length exceeds u32"))?;
            u32::try_from(v.len()).map_err(|_| error("DCS1 value length exceeds u32"))?;
        }
    }
    let payload = match &codec.callbacks {
        Callbacks::Configurations { encode, .. } => {
            encode(&p.main, p.details.as_deref(), p.extra.as_deref())
        }
        Callbacks::State { encode, .. } => encode(p),
    }?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&next.to_le_bytes());
    out.push(match p.mode {
        ChildPanelsMode::Single => 0,
        ChildPanelsMode::Dual => 1,
        ChildPanelsMode::Triple => 2,
    });
    out.push(flags);
    field(&mut out, &payload)?;
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    for (key, state) in children {
        field(&mut out, key.as_bytes())?;
        field(&mut out, &crate::framing::encode(state))?;
    }
    Ok(out)
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NavigationError> {
        let end = self
            .cursor
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| error("truncated DPN1 envelope"))?;
        let b = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, NavigationError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn field(&mut self) -> Result<&'a [u8], NavigationError> {
        let n = self.u32()? as usize;
        self.take(n)
    }
}
pub(super) fn decode<MC, DC, EC>(
    codec: &PanelsCodec<MC, DC, EC>,
    bytes: &[u8],
) -> Result<SavedState<Panels<MC, DC, EC>>, NavigationError> {
    let mut r = Reader { bytes, cursor: 0 };
    if r.take(4)? != MAGIC {
        return Err(error("bad DPN1 magic"));
    }
    let next_key = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
    let mode = match r.take(1)?[0] {
        0 => ChildPanelsMode::Single,
        1 => ChildPanelsMode::Dual,
        2 => ChildPanelsMode::Triple,
        _ => return Err(error("invalid DPN1 mode")),
    };
    let flags = r.take(1)?[0];
    if flags & !3 != 0 {
        return Err(error("invalid DPN1 presence flags"));
    }
    let payload = r.field()?;
    let count = r.u32()? as usize;
    if count != 1 + (flags & 1) as usize + ((flags >> 1) & 1) as usize {
        return Err(error("DPN1 child count does not match presence flags"));
    }
    if count > (bytes.len() - r.cursor) / 16 {
        return Err(error("truncated DPN1 children"));
    }
    let mut children = Vec::with_capacity(count);
    let mut keys = BTreeSet::new();
    for _ in 0..count {
        let key = std::str::from_utf8(r.field()?)
            .map_err(|_| error("invalid UTF-8 DPN1 child key"))?
            .to_owned();
        validate_key(&key, next_key)?;
        if !keys.insert(key.clone()) {
            return Err(error("duplicate DPN1 child key"));
        }
        let state = crate::framing::decode(r.field()?).map_err(|e| error(e.to_string()))?;
        children.push((key, state));
    }
    if r.cursor != bytes.len() {
        return Err(error("trailing bytes after DPN1 envelope"));
    }
    // Structural validation, including every nested frame, precedes user decoding.
    let configurations = match &codec.callbacks {
        Callbacks::Configurations { decode, .. } => {
            let (main, details, extra) = decode(payload)?;
            Panels {
                main: Rc::new(main),
                details: details.map(Rc::new),
                extra: extra.map(Rc::new),
                mode,
            }
        }
        Callbacks::State { decode, .. } => decode(payload)?,
    };
    if configurations.details.is_some() != (flags & 1 != 0)
        || configurations.extra.is_some() != (flags & 2 != 0)
    {
        return Err(error(
            "DPN1 decoded membership does not match presence flags",
        ));
    }
    Ok(SavedState {
        configurations,
        children,
        next_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    const FIXTURE: &[u8] = &[
        68, 80, 78, 49, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 7, 1, 0, 0, 0, 7, 0, 0, 0, 99,
        104, 105, 108, 100, 47, 48, 8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0, 0,
    ];
    fn codec() -> PanelsCodec<u8, u8, u8> {
        PanelsCodec::new(
            |m, d, e| {
                Ok([Some(*m), d.copied(), e.copied()]
                    .into_iter()
                    .flatten()
                    .collect())
            },
            |b| match b {
                [m] => Ok((*m, None, None)),
                [m, d, e] => Ok((*m, Some(*d), Some(*e))),
                _ => Err("payload".into()),
            },
        )
    }
    #[test]
    fn dpn1_has_a_fixed_versioned_main_only_fixture() {
        let p = Panels::<u8, u8, u8> {
            mode: ChildPanelsMode::Triple,
            ..Panels::new(7)
        };
        assert_eq!(
            encode(&codec(), &p, &[("child/0".into(), BTreeMap::new())], 1).unwrap(),
            FIXTURE
        );
        let decoded = decode(&codec(), FIXTURE).unwrap();
        assert_eq!(*decoded.configurations.main, 7);
        assert_eq!(decoded.configurations.mode, ChildPanelsMode::Triple);
        assert_eq!(decoded.next_key, 1);
    }
    #[test]
    fn every_truncated_prefix_and_trailing_byte_fails_before_user_decoder() {
        let calls = Rc::new(Cell::new(0));
        let out = calls.clone();
        let codec = PanelsCodec::<u8, u8, u8>::new(
            |_, _, _| Ok(vec![]),
            move |_| {
                out.set(out.get() + 1);
                Ok((7, None, None))
            },
        );
        for n in 0..FIXTURE.len() {
            assert!(decode(&codec, &FIXTURE[..n]).is_err(), "prefix {n}");
        }
        let mut trailing = FIXTURE.to_vec();
        trailing.push(0);
        assert!(decode(&codec, &trailing).is_err());
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn invalid_modes_flags_counts_keys_counters_lengths_and_nested_frames_precede_codec() {
        let codec = PanelsCodec::<u8, u8, u8>::new(
            |_, _, _| Ok(vec![]),
            |_| panic!("structurally invalid envelope reached codec"),
        );
        for (offset, value) in [
            (0, 0),
            (12, 3),
            (13, 4),
            (19, 0),
            (19, 4),
            (27, 255),
            (33, b'1'),
            (38, 0),
            (42, 1),
        ] {
            let mut b = FIXTURE.to_vec();
            b[offset] = value;
            assert!(decode(&codec, &b).is_err(), "offset {offset}");
        }
        for offset in [14, 23, 34] {
            let mut b = FIXTURE.to_vec();
            b[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(decode(&codec, &b).is_err());
        }
        let mut b = FIXTURE.to_vec();
        b[4..12].copy_from_slice(&0u64.to_le_bytes());
        assert!(decode(&codec, &b).is_err());
    }
    #[test]
    fn duplicate_keys_are_rejected_before_decoding_and_presence_mismatch_after() {
        let p = Panels {
            main: Rc::new(7),
            details: Some(Rc::new(8)),
            extra: Some(Rc::new(9)),
            mode: ChildPanelsMode::Single,
        };
        let children = (0..3)
            .map(|i| (format!("child/{i}"), BTreeMap::new()))
            .collect::<Vec<_>>();
        let mut b = encode(&codec(), &p, &children, 3).unwrap();
        let offset = b.windows(7).position(|w| w == b"child/2").unwrap();
        b[offset + 6] = b'1';
        let never = PanelsCodec::<u8, u8, u8>::new(
            |_, _, _| Ok(vec![]),
            |_| panic!("duplicate keys reached decoder"),
        );
        assert!(decode(&never, &b).is_err());
        let calls = Rc::new(Cell::new(0));
        let out = calls.clone();
        let mismatched = PanelsCodec::<u8, u8, u8>::new(
            |_, _, _| Ok(vec![]),
            move |_| {
                out.set(out.get() + 1);
                Ok((7, Some(8), None))
            },
        );
        assert!(decode(&mismatched, FIXTURE).is_err());
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn encoder_rejects_missing_duplicate_and_noncanonical_child_identities() {
        let p = Panels::<u8, u8, u8>::new(7);
        assert!(encode(&codec(), &p, &[], 1).is_err());
        for (key, next) in [
            ("child/00", 1),
            ("child/0", 0),
            ("stack/0", 1),
            ("child/18446744073709551616", u64::MAX),
        ] {
            assert!(encode(&codec(), &p, &[(key.into(), BTreeMap::new())], next).is_err());
        }
        let p = Panels {
            details: Some(Rc::new(8)),
            ..p
        };
        assert!(
            encode(
                &codec(),
                &p,
                &[
                    ("child/0".into(), BTreeMap::new()),
                    ("child/0".into(), BTreeMap::new())
                ],
                1
            )
            .is_err()
        );
    }
    #[test]
    fn all_presence_patterns_round_trip_in_every_mode() {
        let codec = PanelsCodec::new(
            |m: &u8, d: Option<&u8>, e: Option<&u8>| {
                Ok(vec![*m, d.copied().unwrap_or(0), e.copied().unwrap_or(0)])
            },
            |b| {
                Ok((
                    b[0],
                    (b[1] != 0).then_some(b[1]),
                    (b[2] != 0).then_some(b[2]),
                ))
            },
        );
        for mode in [
            ChildPanelsMode::Single,
            ChildPanelsMode::Dual,
            ChildPanelsMode::Triple,
        ] {
            for d in [None, Some(8)] {
                for e in [None, Some(9)] {
                    let p = Panels {
                        main: Rc::new(7),
                        details: d.map(Rc::new),
                        extra: e.map(Rc::new),
                        mode,
                    };
                    let count = 1 + usize::from(d.is_some()) + usize::from(e.is_some());
                    let children = (0..count)
                        .map(|i| {
                            (
                                format!("child/{i}"),
                                BTreeMap::from([("nested".into(), vec![23, i as u8])]),
                            )
                        })
                        .collect::<Vec<_>>();
                    let b = encode(&codec, &p, &children, count as u64).unwrap();
                    let restored = decode(&codec, &b).unwrap();
                    assert_eq!(*restored.configurations.main, 7);
                    assert_eq!(restored.configurations.details.as_deref().copied(), d);
                    assert_eq!(restored.configurations.extra.as_deref().copied(), e);
                    assert_eq!(restored.configurations.mode, mode);
                    assert_eq!(restored.children, children);
                }
            }
        }
    }
}
