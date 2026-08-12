# Plans31–33 final route-gate handoff — 2026-08-12 17:35 HKT cutoff

## Completed numbered plans

- Plan31 is complete in `beecd55` and `6bfd5b0`. Its debug and release
  `plan31_authoritative_block_actions` vectors each passed 3/3. The read-only
  `luna_worker` audit found no remaining Plan31 TCP routing/revision/replication blocker.
- Plan32 is complete in `4b02aa0`, with the roadmap status closed separately in
  `68d0063`. Its debug and release `plan32_progression_travel` vectors each passed 5/5.
- Plan33 is complete in `260f158`. Fresh TCP gameplay input is rebound to the latest owner
  revision immediately before egress, autonomous fixed ticks no longer advance the
  client-authored revision baseline, and explicit stale/out-of-order probes remain unchanged.
  The new Embedded/Listen/Dedicated lifecycle vectors passed 3/3 in debug and release; the Plan30
  regression matrix passed 2/2 in debug and release.
- Plan34 remains the already-numbered, out-of-scope container-break inventory-conservation gap. No
  additional numbered Plan was needed while completing Plans31–33.

## Final route-gate state at cutoff

The requested exact debug command was run serially:

```text
cargo test --locked --no-fail-fast -- --test-threads=1
```

It exited 1 after 1,198.6 seconds. Because `--no-fail-fast` completed the target traversal, its
final summary identified exactly three failed targets:

```text
--lib
--bin icraft
--test headless_server_authority
```

The Plan30, Plan31, Plan32, and Plan33 integration targets were not among the failed targets. The
captured top-level output was truncated before the individual failing test names/assertions. A
serial diagnostic rerun of `cargo test --locked --lib --no-fail-fast -- --test-threads=1` was started
with a filtered log, but was intentionally terminated before completion to honor the user-imposed
17:35 HKT stop-and-push deadline; no failure pattern had appeared before termination.

The release full suite, release check, and post-suite fmt/diff gates were therefore not run and must
not be claimed as passing. Before the full-suite attempt, Plan33's targeted fmt and diff checks did
pass.

## Exact resume order

1. Re-run the three failed debug targets individually with `--test-threads=1`, retaining full logs,
   and identify the concrete test cases/assertions.
2. Determine whether each failure is deterministic, environmental/flaky, or a regression. Add a
   numbered Plan before any real newly discovered scope; do not fold unrelated fixes into Plan33.
3. After any scoped correction, rerun the exact serial debug full suite, then the serial release full
   suite, release check, `cargo fmt --all -- --check`, `git diff --check`, and staged/working-tree
   diff gates.

## Preserved user work

`Cargo.toml`, `assets/texture_atlas.png`, and `controls.config` were already modified by the user.
They remain unstaged and are intentionally excluded from all Plans31–33 and handoff commits.
