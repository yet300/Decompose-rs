# ADR-0014: Selective persistence for standard routers

Date: 2026-10-10. Status: accepted.
Reference: Kotlin Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490.

## Context

Generic Navigation already permits optional encoding and restoration. The five
standard router codecs return nonoptional results, so existing applications
cannot skip a single save or reject a valid saved state. Changing their callback
return types would break source compatibility and could confuse empty membership
with rejection.

Upstream Stack, Pages and Panels factory callbacks return nullable saved or
restored states. Slot wraps a nullable restored configuration in a nonnull
SlotNavState: null configuration restores an empty slot. Items accepts an optional
NavStateSaver. These are distinct upstream contracts, rather than five identical
callback signatures.

## Decision

Add `should_save` and `should_restore` builder predicates to StackCodec,
SlotCodec, PagesCodec, PanelsCodec and ItemsCodec. They default to acceptance and
are shared by cloned codecs. Stack receives its ordered configuration slice;
Slot receives `Option<&C>`, including None for an empty slot; the other families
receive their whole Pages, Panels or Items navigation state, including selection,
mode or active targets. The existing encode/decode constructors and callbacks are
unchanged and no bounds on configurations are added.

Save evaluates the predicate on each navigation snapshot before invoking child
state providers or encoding. A false result omits the router's key from that
save. It does not remove the provider and a later save can include the same
router. Callback panics propagate under the existing panic contract.
These predicates configure the byte codec. Explicitly selecting the independent
in-memory navigation saver bypasses byte-codec callbacks and predicates.

Restore first validates the complete envelope, every nested child frame,
configuration payload, membership rules and stable child identities. Only then
does it evaluate the predicate. A false result invokes the initializer exactly
once and begins without the saved child states or their retained scopes. An
accepted restoration never invokes the initializer. No saved state also invokes
the initializer but never invokes the restore predicate. Valid empty Slot, Pages
and Items state is still a successful restoration. Stack must remain nonempty;
Panels must always have Main.

Construction rollback returns consumed bytes even when a declined restoration's
initializer or child factory fails. Retention ownership follows the shared engine
transaction: accepting the state matches scopes by stable saved keys; successful
decline discards old retained scopes. Failed construction restores the prior
retention map but cannot resurrect objects already destroyed by user callbacks
or by scope disposal. The shared engine owns these lifetime guarantees.

## Compatibility and evidence

The DST1, DSL1, DPG1, DPN1 and DPI1 byte envelopes are unchanged. Old codecs accept
all valid restores and participate in every save. Existing byte fixtures and
family persistence tests continue exercising that contract. These formats are
Rust-specific; this change does not import Kotlin SerializableContainer bytes.

`tests/selective_persistence.rs` exercises all five families: repeated skip/accept
saves without child-provider calls on skip, absent versus empty versus declined
state, lazy initialization, nested state and stable keys, retained instances,
every truncated saved-frame prefix, and retry after failed declined construction.

The Slot predicate deliberately adds explicit rejection separately from a valid
empty slot. This is the Rust interface requested for all standard routers; it
does not pretend that upstream Slot's null configuration denotes rejection.
