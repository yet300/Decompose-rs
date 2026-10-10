# ADR-0018: Whole-state callbacks for standard persistence

Status: Accepted. Pinned reference:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.

## Audit finding

Upstream Pages `savePages/restorePages`, Panels `savePanels/restorePanels` and
Items `NavStateSaver<Items<C>>` expose the whole model state, rather than only
configurations. A caller may keep valid ordered membership while restoring a
different selected page, active-item map or panel mode. Selective predicates
cannot replace that capability: declining restoration loses the saved child
subtree and invokes the initializer. The initial compatibility matrix and
list/role codecs did not expose these metadata transformations.

Upstream Slot `restoreConfiguration` returning null represents an accepted
empty slot, including when the saved slot was populated. It is different from
returning no restored navigation state and invoking the initializer.

## Decision

Add `new_state` constructors to PagesCodec, ItemsCodec, PanelsCodec and SlotCodec.
Pages/Items/Panels accept caller byte encoders/decoders for the whole public
model state; Slot uses a populated-configuration encoder and nullable decoder.
All old `new` constructors, builder policies, complete options literals and
Rust envelopes remain unchanged. No dependency or configuration Clone/Hash bound
is added; opaque application payload bytes are still caller-owned.

For Pages/Items/Panels, the durable envelope retains its original metadata;
whole-state callbacks encode their own payload with any required model metadata.
All original envelope/framing validation precedes caller decoding, including
selection/status/mode bytes, keys, counts and nested frames. The resulting
model is normalized/validated before factories: correct selection, activity keys
outside membership removed under the existing Items contract, uniqueness, ordered child count and typed panel-role presence. The decoder's
whole-state metadata takes precedence over the envelope defaults. The caller
must preserve configuration order/identity required by saved child ownership.
An old list/role decoder continues using the original envelope metadata.
These ownership constraints follow the pinned public contracts: Stack
ChildStackFactory.kt requires the same amount/order; Pages ChildPagesFactory.kt
requires the same amount/order; Panels ChildPanelsFactory.kt requires exactly
the same configurations. They are not unsupported valid cardinality transforms.

A whole-state Slot decoder returns `Option<C>`: None accepts empty membership
and does not invoke the lazy initializer. When it empties a populated save,
saved child state is discarded and the allocation counter stays monotonic.
Saved empty DSL1 slots contain no payload and skip both callbacks, preserving
the existing format and accepted-empty behavior. Existing SlotCodec::new returns
C for a populated payload and keeps the historical format/behavior.

`should_save` runs before either encoder and nested providers; `should_restore`
runs after successful full validation and may decline. Thus absence, decline,
accepted empty state, transformed metadata and corruption remain distinct.
Explicit memory navigation selection continues to bypass byte callbacks/policies.
Construction rollback returns the original saved bytes, including decoder,
validation and factory failures, allowing a corrected retry.

## Evidence

`tests/whole_state_codecs.rs` exercises metadata replacement while retaining
nested child bytes and stable keys, legacy-to-whole decoding, invalid selection,
activity/membership/count/presence, corruption before callback entry, codec and
factory failure retry, empty/inactive states, policy ordering and no-extra Panels.
Slot regressions cover accepted empty membership without initializer.
`examples/whole_state_persistence.rs` is runnable; the full suite keeps all
old-format fixtures and non-Clone configuration tests. Final evidence is in
the completion plan and COMPATIBILITY.md. Kotlin saved bytes are not compatible
without an explicit migration.
