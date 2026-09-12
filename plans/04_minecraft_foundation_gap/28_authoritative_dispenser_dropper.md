# Plan 28 — authoritative Dispenser/Dropper automation

## Scope

This plan closes the authoritative runtime gap between redstone's existing
rising-edge `Dispense` action and the CPU/server world.  Each action is drained
once, sorted by source position/facing, and validated against the loaded source
block, source block entity, and loaded front chunk before any item is consumed.
The supported matrix is intentionally narrow: Arrow, Splash Potion, water/lava
bucket transitions into/out of air, Flint and Steel fire placement, and ordinary
stack fallback drops.  Droppers insert one item into the loaded target using
merge-first/lowest-empty deterministic selection and otherwise create one
metadata-complete dropped stack.

Plan27 and Plan28 finalize one not-yet-published protocol-v17 development
sequence. Dropped-item entity projections carry the complete item metadata
needed by clients; no further bump is made. The intermediate Plan27 wire shape
is not compatible and is not presented as a supported peer.
Entity ids are allocated by `AuthorityCore`, not by per-world entity managers.

## Persistence and convergence contract

Container slots/revisions and dropped-stack metadata round-trip through the
existing chunk/entity save formats.  Redstone's `last_powered` latch is part of
the persisted component sidecar so a powered reload does not create a phantom
edge.  Source and target block-entity revisions are published as ordinary world
mutations.  Headless authority and two-client/runtime-topology tests assert the
same source/target state, dropped metadata, entity ids, and checksum.

## Non-goals

Full vanilla dispenser behaviour, cauldron or waterlogging integration, hopper
rewrites, GPU/window/audio/manual visual evidence, and any new protocol version.

## Verification record

Current targeted evidence (working tree, 2026-08-12):

- `cargo test --lib authoritative_`: 10 passed (including global-id,
  deterministic matrix, unloaded-front, and Dropper merge/fallback vectors).
- `cargo test --lib bucket_`: 2 passed; `powered_dispenser_latch_roundtrips_without_phantom_edge`:
  1 passed; current `redstone_metadata_sidecar` and
  `legacy_redstone_sidecar_preserves_fields_and_defaults_latch`: 1 each;
  `entity_lifecycle_and_player_authority_roundtrip`: 1 passed.
- `cargo test --test headless_server_authority
  tcp_dispenser_drop_projection_converges_complete_item_metadata`: 1 passed.
  Two real TCP clients receive the same DroppedItem id/metadata and matching
  source BlockEntityDelta revision/slot; a powered reset→edge then proves
  Dropper→Chest source decrement + target merge on both clients with no new
  fallback entity.
- `cargo test --test runtime_topology_parity
  plan28_dispenser_item_projection_matches_all_runtime_topologies`: 1 passed
  across Singleplayer, ListenServer, and Dedicated; `cargo fmt --all` completed.

Final serial full-suite evidence (2026-08-12, `--test-threads=1` to isolate
the existing process-local save-failure injection test) passed in both debug
and release: 1,524 passed, 0 failed, 6 ignored across the 684-test library
lane, 815-test client binary lane, 2-test server binary lane, seven integration
lanes (3/3/3/2/1/6/5), and zero doc-tests. `cargo check --all-targets` and
`cargo check --release --locked` also pass; warnings are pre-existing and
unrelated to this plan. Plan14 was safely edited as UTF-8; the older Plan02
file retains its pre-existing invalid UTF-8 control byte and its historical
checkbox was not rewritten.
