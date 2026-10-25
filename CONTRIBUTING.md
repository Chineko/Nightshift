# Contributing to Nightshift

Nightshift favors small, proven changes over broad rewrites. Build the basement before the roof: migrate one coherent slice end-to-end, verify it, then expand.

## Integration-test module graph rule

Several tests under `crates/nightshift-overlay-prototype/tests/` use `#[path = "../src/...rs"] mod ...;` to compile production source files directly into standalone integration-test crates.

That has an important consequence: **each `tests/*.rs` file is its own crate root**.

If a production file included by one of those tests imports another module through `crate::foo`, that integration-test crate must declare a matching `foo` module itself. Transitive dependencies matter too.

Example from the Taffy brightness migration:

```text
interaction.rs
  -> crate::home_layout
       -> crate::layout
            -> crate::widget
```

Any integration test that includes `interaction.rs` (directly or through another included production module) must therefore make `home_layout`, `layout`, and `widget` available at its own crate root, for example:

```rust
#[path = "../src/home_layout.rs"]
mod home_layout;
#[path = "../src/layout.rs"]
mod layout;
#[path = "../src/widget.rs"]
mod widget;
```

### Required check when adding a new `crate::...` dependency

Before relying on CI to discover failures:

1. Trace the full `crate::...` dependency chain introduced by the change.
2. Search every `tests/*.rs` harness that includes the changed source file, directly or indirectly.
3. Update every affected standalone test crate in the same change.
4. Inspect the complete diff for unrelated deletions or whole-file replacement collateral.
5. Run the full CI gate before promotion.

Do not fix only the first compiler error and then wait for CI to reveal the next identical one. **Find the failure class and audit every place where that class can occur.**

This rule exists because the first Taffy basement migration lost time by treating repeated unresolved-module errors as isolated CI failures instead of auditing the integration-test module graph once.

## UI renovation rule

For the overlay rebuild/replacement work:

- keep the existing renderer, interaction semantics, application state, OpenXR plumbing, COSMIC Text, effects primitives, and Hydra behavior unless a slice specifically requires changing them;
- replace layout geometry one component at a time;
- let Taffy be the single geometry source for both rendering and hit-testing in each migrated component;
- avoid compatibility layers that preserve a second geometry ledger;
- do not expand to the next component until the current slice is green in CI;
- verify the full branch diff against the known-good base before promotion.

Hydra is intentional. Preserve the easter eggs and playful comments where they do not obscure what the code does. We can keep the crayons; Hydra just gets a ruler.
