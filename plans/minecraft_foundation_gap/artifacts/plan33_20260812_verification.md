# Plan33 headless verification — 2026-08-12

## Acceptance evidence

- A fresh TCP gameplay input receives its sequence and latest owner revision immediately before
  socket egress. Explicit stale/out-of-order inputs retain their caller-supplied sequence and
  revision so the authority gates remain testable and enforced.
- Autonomous fishing/cooldown/brewing fixed ticks update the owner-private gameplay projection but
  no longer invalidate an in-flight client request by changing the client-authored revision
  baseline.
- Embedded, Listen TCP, and Dedicated TCP vectors prove cast-to-nibble-to-reel, hook removal,
  inventory loot, XP gain, fishing-rod durability, cached duplicate idempotency, cancellation,
  owner-only session projection, and continued stale/out-of-order rejection.
- The pre-existing Plan30 TCP matrix now expects and receives a successful fresh reel instead of
  preserving the former deterministic `InvalidRevision` blocker.

## Commands

```text
cargo test --locked --lib network::client::tests::fresh_gameplay_input_rebases_revision_but_explicit_stale_input_does_not -- --test-threads=1
cargo test --locked --test plan33_tcp_fishing_lifecycle -- --test-threads=1
cargo test --locked --release --test plan33_tcp_fishing_lifecycle -- --test-threads=1
cargo test --locked --test plan30_real_transport_acceptance -- --test-threads=1
cargo test --locked --release --test plan30_real_transport_acceptance -- --test-threads=1
cargo fmt --all -- --check
```

The Plan33 vectors passed three tests in both debug and release. The Plan30 regression matrix passed
two tests in both profiles. Formatting passed. Repository-wide serial debug/release suites, release
check, and final diff gates remain reserved for the post-Plan33 route gate and are not claimed here.
