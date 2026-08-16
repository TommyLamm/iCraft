# Plan31 headless verification — 2026-08-12

## Corrective acceptance evidence

- `tests/plan31_authoritative_block_actions.rs` keeps owner/observer positions in chunk `(0,0)`
  while mining, placement, block-entity, drop, and persistence targets reside in chunk `(1,0)`.
- The real TCP vector starts an unfinished Obsidian mining action, observes owner-private mining
  progress, disconnects the owner socket, waits for authority cleanup, reconnects the same username
  as a new player id, and asserts the replacement session has no mining progress.
- Embedded, Listen TCP, and Dedicated TCP share the same bounded block-action assertions.

## Commands

```text
cargo test --test plan31_authoritative_block_actions -- --nocapture --test-threads=1
```

Result: 3 passed, 0 failed. The formatting gate was run after applying `cargo fmt --all`.

The repository-wide serial debug/release suites are recorded at the final route gate after
Plans32–34, so this artifact does not duplicate or overstate that later evidence.
