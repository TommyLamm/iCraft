# Plan34 verification — 2026-08-13

## Scope

Focused Plan34 evidence only. No repo-wide full-suite result is claimed. Existing
dirty user files `Cargo.toml`, `assets/texture_atlas.png`, and `controls.config`
were not changed.

## Production seam

`src/authority/mod.rs::AuthorityCore::commit_mining_break` now snapshots non-empty
slots from Chest (27), Furnace (3), Hopper (5), Dispenser (9), and Dropper (9)
after session/tool/XP preflight and before `set_block(..., Air)`. Block drops and
container stacks reserve entity IDs in one list and prepare all `DroppedItem` entities
before the Air mutation. Any prepare or mutation failure removes the prepared entities
and leaves the source block entity intact; only a successful Air mutation commits the
pending mutation and session result. No pre-mutation drain or clear is performed;
`set_block` removes the block entity as before and runtime projection reuses
`BlockEntityDelta(None)` and `EntityStateWire.item`. Save/reload retains the exact
full `ItemStack` multiset, total count, and multiplicity; no entity ID is compared
across save/reload, while owner and observer IDs are compared only within the same
runtime. The legacy raw-bincode entity save does not claim stable IDs across restart.

## Focused commands

- `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1` — pass (authority five-kind matrix with cached/stale/post-commit fresh retries, double-chest half isolation, stale-state atomic failure, and real Listen/Dedicated TCP projection/reconnect/save-reload; 4 tests).
- `cargo test --release --test plan34_container_break_inventory_conservation -- --test-threads=1` — pass (the same 4 focused tests in release).
- `cargo test --test plan31_authoritative_block_actions -- --test-threads=1` — pass (3 Plan31 embedded/Listen/Dedicated regression tests).
- `cargo test --release --test plan31_authoritative_block_actions -- --test-threads=1` — pass (the same 3 Plan31 regression tests in release).
- `cargo fmt --all -- --check` — pass.
- `git diff --check` — pass.

The existing `authority::tests::typed_mining_fixed_tick_breaks_once_and_cancel_is_idempotent`
focused regression also passed after the seam change. Compilation emits existing
unused-code warnings only.

## Remaining boundary

This closes the numbered Plan34 acceptance in the bounded authority/TCP scope. It does
not claim complete vanilla drop scatter/merge physics, GPU/manual visual evidence, or a
repo-wide full-suite rerun.
