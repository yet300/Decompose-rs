# ChildItems performance and equality contract

The default Eq-only API remains available without `Hash`, `Ord`, or `Clone` configuration bounds. Membership is ordered, unique by `Eq`, and child identity follows equality. Default navigation revalidates uniqueness on every update, including after interior mutation of identity fields. Default maps and child matching retain their earliest equal candidate semantics. Nonidentity data may change, and navigation may replace a configuration with a freshly allocated equal value. Such replacements retain the component and use the current configuration payload when recreating it.

An explicitly indexed update promises stable equality identity while configurations are stored. Only with that opt-in can an unchanged update be recognized by comparing each new configuration with the corresponding previously validated configuration. This is a value equality test, including for freshly allocated equal configurations. It reuses the committed uniqueness invariant, avoiding another pairwise duplicate scan. Initial and restored input, changed membership, and reorders undergo duplicate validation. Without this contract, shared Rc configurations can become equal between events, so caching default uniqueness would silently accept duplicates; a regression reproduces and prevents that failure.

`ItemMap` maintains insertion order. In the stable indexed path a shared handle selects a lookup candidate using its address, but the candidate is still checked with `Eq`; fresh allocations use fingerprint buckets. Address lookup is an accelerator, not an alternative equality relation. Default maps preserve a linear earliest-equal lookup even after interior mutation. Normalization and snapshot construction use already unique, canonical membership to build maps directly instead of reinserting every distinct configuration through a linear lookup. Engine matching uses same-position equality only with the stable opt-in and optional fingerprint buckets for changed membership. Default models, including stacks permitting duplicates, preserve their earliest unmatched equality policy. Lifecycle target and retention lookups use ordered indexes instead of repeatedly scanning all next children.

## Optional index

Use `ConfigurationIndex::new(|configuration| stable_u64_identity)` for an Eq-only configuration with a suitable equality-compatible identity, or `ConfigurationIndex::hashed()` when `C: Hash`. Equal configurations must produce equal fingerprints. Both fingerprints and equality identity must remain stable while configurations are stored. Different configurations may collide: buckets always use `Eq` to distinguish entries and reject duplicates. `ConfigurationIndex::equality_only()` explicitly promises stable equality without Hash or a key callback; it uses a constant fingerprint, giving the unchanged fast path while preserving quadratic changed-list fallback.

With dispersed fingerprints, `Items::new_indexed` avoids quadratic initial duplicate validation. Pass the same policy to `child_items_indexed` or `child_items_with_initial_indexed`. `ItemMap::with_index` supports efficient preparation of large active-target maps. The index is used by validation, normalization, published active maps, and engine child matching; it is not confined to a standalone map. Existing `ItemsOptions` struct literals and Eq-only constructors remain source-compatible. No index data is persisted, and the existing DPI1 persistence format is unchanged.

## Deterministic evidence

`tests/items_performance.rs` instruments only configuration `PartialEq`, using a configuration that implements neither `Hash` nor `Clone`. The audited pre-fix inactive measurement was reproduced before implementation with a temporary baseline assertion of `n * (n - 1) / 2`, and the linear regression failed at 32,640 comparisons for 256 items. The permanent tests enforce linear comparison bounds and separately exercise fresh equal allocations, full fingerprint collisions, duplicate rejection, map replacement/removal, indexed initial validation, lazy activation, lifecycle destruction, saved tombstones, restored state, and correct map lookups after caught predicate/fingerprint panics.

Measured Eq calls for one update, excluding initial construction:

| Items | Before and current default: unchanged inactive | After: unchanged inactive, stable Eq-only opt-in | After: unchanged all active, stable Eq-only opt-in | After: all active reverse reorder with fresh equal allocations, dispersed index |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 32,640 | 256 | 1,024 | 1,153 |
| 512 | 130,816 | 512 | 2,048 | 2,305 |
| 1,024 | 523,776 | 1,024 | 4,096 | 4,609 |

Run the regression and print actual measurements:

```sh
rtk proxy cargo test --test items_performance --locked -- --nocapture
```

The all-active reorder test verifies that a surviving child retains its exact `Rc` component identity after reversing the list. Factory, lifecycle, observer and completion code still runs according to the existing navigation contract; no-op updates still publish and complete.

## Remaining complexity

- The default Eq-only path revalidates unique membership on each update and retains O(n²) duplicate/matching work, including unchanged inactive updates. General subquadratic uniqueness cannot be guaranteed from an equality oracle with mutable identity; the default behavior is preserved rather than silently weakening duplicate validation.
- Stable Eq-only opt-in unchanged inactive membership has O(n) router work and Eq calls. Unchanged canonical active membership has O(n) Eq calls, with O(n log n) scope/retention bookkeeping. Fresh equal active handles can still require linear collision-bucket equality lookups per active entry with the constant fingerprint policy.
- With dispersed fingerprints, indexed duplicate validation, map construction/lookups and child matching perform expected O(n) total equality work per list update. Scope/retention bookkeeping is O(n log n). Complete fingerprint collisions preserve correctness but can recover O(n²) equality work.
- A caught map predicate/fingerprint panic invalidates cached indexes before mutation; lookups fall back to equality scans until a successful rebuild or clear.
- Ordered map removal and index rebuilding are O(n); indexed insertion and lookup are expected O(1) plus collision-bucket equality checks. Constructing a distinct Eq-only map through public repeated insertion remains O(n²).
- Lazy member lookup and engine liveness checks can scan membership/keys. These changes do not claim constant-time end-to-end `get_or_create` or bounds on arbitrary user callbacks and Essenty internals.
