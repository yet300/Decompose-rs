# ChildPanels Rust design

Status: implemented; independent specification and quality reviews passed.
Authorized by the user's continuation to the proposed ChildPanels stage.
Reference: local upstream-ref at 00840bcfed6d55710c8fbb1a72503f2041eda490,
router/panels and its tests. Keep one crate above published Rust Essenty. Exclude
PanelsWebNavigation and all UI/platform adapters. Preserve the 221-test baseline
and old Stack/Slot/Pages public APIs and DST1/DSL1/DPG1 persisted bytes.

## Typed public API

Panels has three distinct configuration types; ChildPanels has three distinct
component types. Do not collapse these to one enum supplied by callers or Any.
Use private role-tagged configuration/component enums to reuse the common engine.
Role tags are part of identity even when configuration types and values coincide.

`Panels<MC, DC, EC = Infallible>` has main: Rc<MC>, details: Option<Rc<DC>>,
extra: Option<Rc<EC>>, mode: ChildPanelsMode. Main is always present. `Panels::new`
accepts an owned main and defaults to no optional panels and Single mode; fields
remain public for transforms. Manual Clone requires no configuration Clone.
`ChildPanels<MC, MT, DC, DT, EC, ET>` contains typed Child main, optional typed Child
details/extra and mode, with manual Clone and no component Clone requirement.

ChildPanelsMode is Single (default), Dual or Triple. `PanelsNavigation<MC, DC,
EC = Infallible>` is a shared synchronous source, manually Clone without bounds.
It provides navigate/navigate_with over actual Panels state, set_panels(main,
details, extra), set_details_extra(details, extra), set_extra(extra),
activate_main/details/extra, dismiss_details/extra, pop and set_mode. Each helper
has a _with completion exposing Result<NavigationResult { old_panels, new_panels },
NavigationError> per connected receiver. Ownership arguments become Rc once at
submission so events can broadcast without Clone. Convenience failures panic,
as with the existing routers. Equal role/configuration activation is an identity-
preserving no-op that still publishes and completes.

PanelsOptions contains a unique hosting key (DefaultChildPanels),
handle_back_button (false), codec (None), and optional on_state_changed callback
`Fn(&Panels, Option<&Panels>)`. It runs initially with old=None, then after snapshot
publication and automatic Back enablement, before event completion. Automatic
Back invokes it too. No-op events invoke it. Reentrant callback events are queued;
callback panic propagates while queues recover. Completed state/callback effects
are not rolled back. Initial callback runs with provider and Back hook installed,
while construction's queue is still held, and construction rollback applies.

`child_panels` takes context, navigation, initial Panels, options and three typed
factories, returning Result<Value<ChildPanels>, NavigationError>. A named
`child_panels_without_extra` helper maps Rust Infallible to Kotlin Nothing and
takes just Main/Details factories; the Extra component/configuration types are
Infallible. No fictitious Extra component or factory invocation is allowed.
Keep custom context factory propagation and Eq + 'static configuration bounds;
no Clone/Send/Sync, runtime or serde dependency.

## Lifecycle and navigation semantics

Every present panel remains live. Mode changes never recreate components.

| Presence | Single | Dual | Triple |
|---|---|---|---|
| Main alone | Main Resumed | Main Resumed | Main Resumed |
| Main + Details | Details Resumed; Main Created | Both Resumed | Both Resumed |
| Main + Extra | Extra Resumed; Main Created | Both Resumed | Both Resumed |
| All three | Extra Resumed; Main/Details Created | Main/Extra Resumed; Details Created | All Resumed |

Main/Details/Extra factory creation is forward order; destruction is reverse.
Demote outgoing live panels before incoming promotions. Parent lifecycle bounds
every panel and Back bridge. Equality within the same role retains component,
keeper and key. Removing/replacing one role ends only its outgoing scope; other
roles survive. Removing Details leaves Extra unchanged, and replacing Main
leaves Details/Extra unchanged. Explicit pop removes Extra first, then Details,
and is a no-op with Main alone, regardless of mode.

Automatic Back is enabled only in Single with optional panels present and
handle_back_button=true. It removes Extra first, otherwise Details. In Dual or
Triple it does not consume Back. Enabled live child handlers still have higher
priority; Created children cannot handle Back. Preserve SharedBack priority/LIFO,
predictive cancellation, deferred navigation and panic recovery.

## Engine integration and persistence

Keep one transaction/source/scope engine. Generalize private configurations()
to produce a Vec<Rc<TaggedConfiguration>> so typed Panels state needs no hidden
cache or selection side channel. Existing model policies clone configuration
handles. Add a default no-op model state-changed hook, with explicitly ordered
initial and navigation notification; other models keep their public behavior.
Keep prepare-before-destruct, reservations, ownership guards, live-scope
liveness checks, external retention, coherent saves and reentrant queues.

PanelsCodec has two typed whole-membership callbacks: encode(&MC, Option<&DC>,
Option<&EC>) -> Result<Vec<u8>, String>, and decode(&[u8]) -> Result<(MC,
Option<DC>, Option<EC>), String>. Mode is encoded by the library. `without_extra`
adapts two-role callbacks to PanelsCodec<MC, DC, Infallible>. Manual Clone has no
configuration bounds. Absent optional roles invoke no component factory.

Use a distinct DPN1 envelope: mode, optional-role presence, configuration codec
payload, ordered child key/state records, next-key counter. Validate mode,
presence flags, role/count consistency, framing, canonical unique keys/counter,
nested DCS1 and trailing bytes before codec/factories; decoded optional membership
must match envelope presence. Corruption returns a typed error without fallback.
Restored mode/membership override initial state. Preserve nested states for each
typed role, external retained service identity, and retryable original input.
No codec means transient navigation and no child-provider calls on parent save.
Save a coherent membership/mode/key/counter snapshot before providers can navigate.

## Acceptance

Tests cover all twelve mode/presence combinations and transitions between modes;
typed non-Clone heterogeneous configs/components/custom contexts; same values
across roles; identity-preserving equal operations; independent replacement and
dismissal; factory/destruction/lifecycle order; all navigation helpers and callback
ordering/reentry; child/automatic Back and predictive cancellation; persistent
mode/role/child-state restore, external retention/transient reset; malformed frames,
codec errors, key reservation/type conflict/input rollback; constructor/factory/
lifecycle/state-hook/observer/completion panics and usable retries; shared sources,
mixed/nested routers and view/source/drop cleanup. Keep all 221 baseline tests
green and immutable old-format fixtures. Provide example, README, ADR and roadmap,
independent spec then quality review, strict fmt/Clippy/rustdoc, all-target/doc tests,
four runnable router examples and compilation across all 24 installed targets.
