# Plan32 headless verification — 2026-08-12

## Acceptance evidence

- Singleplayer submits typed flint-and-steel portal activation and typed portal entry through the
  embedded authority request queue.
- Listen and Dedicated vectors use two real TCP clients. They prove cached duplicate ACK behavior,
  stale-revision rejection, owner-private dimension transfer, observer privacy, and persisted
  Nether dimension after disconnect/reconnect.
- Dedicated TCP uses typed operator commands, pose packets, and combat requests to defeat a
  generated Ender Dragon. Authority creates the exit fountain, egg, and gateway.
- Generated Nether fortress and End City chests materialize loot lazily, allocate revisions, and
  retain the same inventories after save/reload.

## Commands

```text
cargo test --test plan32_progression_travel -- --test-threads=1
cargo test --release --test plan32_progression_travel -- --test-threads=1
cargo check --all-targets
cargo check --release --locked
cargo fmt --all -- --check
git diff --check
```

Both test profiles passed 5 tests with zero failures. Both compile checks passed. Formatting and
diff checks passed after the documentation update. Repository-wide serial suites remain reserved
for the final Plans31–34 route gate and are not claimed here.
