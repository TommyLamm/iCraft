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

### Bounded diagnosis completed before cutoff

A standalone serial rerun of `cargo test --locked --test headless_server_authority --no-fail-fast
-- --test-threads=1` reproduced one failure (1 passed, 1 failed). The concrete case is
`two_clients_share_headless_authority_with_revision_interest_and_reconnect`, at
`tests/headless_server_authority.rs:486`: after resending the same `BLOCK_REQUEST`,
`alice.take_response(BLOCK_REQUEST)` returned a replayed cached response even though the test
requires the client response gate to suppress it. The adjacent authority metrics assertions are
intended to prove the replay is counted once without accepting/rejecting or executing the mutation
again. This is close to the Plan33 `NetworkClient` seam and is a real reproducible blocker, but the
cutoff arrived before the response-gate call path could be audited far enough to establish whether
it is a Plan33 regression or a pre-existing expectation conflict. Do not patch it speculatively;
make that determination first, and create the next numbered Plan if the missing behavior is outside
Plan33's stated revision-lifecycle scope.

CodeGraph and `git blame` narrowed this further before cutoff. Plan31 commit `beecd55` deliberately
changed `GameplayResponseGate::accept` so a byte-for-byte identical cached ACK returns `true` and is
surfaced again; its unit test
`gameplay_response_gate_replays_exact_cached_ack_and_drops_rewrites` explicitly requires that
behavior. The failing headless integration explicitly requires the opposite behavior for the same
active-client replay. This is therefore an unresolved Plan31 transport/replication contract conflict,
not evidence that Plan33's revision rebasing caused the failure. Do not make only one assertion
green: first define one coherent contract for retry completion versus already-observed replay
suppression, then update implementation and both levels of tests together. Because this conflict is
inside Plan31's stated seam, no new numbered Plan was created merely to defer it.

## Resumed resolution — 2026-08-13

The conflict was resolved inside Plan31 without adding a numbered Plan. Plan30 already defines the
two-layer contract: the server replays its byte-identical cached ACK and records one duplicate, while
`NetworkClient` suppresses a response for a request id already delivered into its reliable app
queue. If the original network ACK never reaches the client gate, the first copy that does arrive is
still accepted normally. Plan31's conflicting gate unit and TCP duplicate vector were corrected to
that contract; the integration assertion was retained unchanged.

Focused resumed evidence:

```text
cargo test --locked --lib network::client::tests::gameplay_response_gate_drops_exact_cached_ack_and_rewrites -- --test-threads=1
cargo test --locked --bin icraft network::client::tests::gameplay_response_gate_drops_exact_cached_ack_and_rewrites -- --test-threads=1
cargo test --locked --test headless_server_authority --no-fail-fast -- --test-threads=1
cargo test --locked --release --test headless_server_authority --no-fail-fast -- --test-threads=1
cargo test --locked --test plan31_authoritative_block_actions -- --test-threads=1
cargo test --locked --release --test plan31_authoritative_block_actions -- --test-threads=1
cargo test --locked --test plan30_real_transport_acceptance -- --test-threads=1
cargo test --locked --release --test plan30_real_transport_acceptance -- --test-threads=1
```

These passed 1/1, 1/1, 2/2, 2/2, 3/3, 3/3, 2/2, and 2/2 respectively. The previously failed full-suite
targets must still be re-run through the exact final serial route gates before claiming completion.

## Exact resume order

1. Rerun `--lib` and `--bin icraft` individually with `--test-threads=1`, retaining full logs to
   identify any remaining concrete test cases/assertions after the Plan31 corrective.
2. Determine whether any remaining failure is deterministic, environmental/flaky, or a regression. Add a
   numbered Plan before any real newly discovered scope; do not fold unrelated fixes into Plan33.
3. After any scoped correction, rerun the exact serial debug full suite, then the serial release full
   suite, release check, `cargo fmt --all -- --check`, `git diff --check`, and staged/working-tree
   diff gates.

## Preserved user work

`Cargo.toml`, `assets/texture_atlas.png`, and `controls.config` were already modified by the user.
They remain unstaged and are intentionally excluded from all Plans31–33 and handoff commits.
