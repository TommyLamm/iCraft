# `repro_catchup_mailbox_full_drops_chunk`

> Status: open structural/slow-client reproduction
> Input: seed `42`, one host, one joining slow client, catch-up capacity `32`
> (the R3 regression must also run with capacity `1`)
> Repair round: R3

## Evidence

- `src/state.rs:4690-4701` — `process_join_catchups` removes a coordinate with
  `pop_front()` before the server accepts ownership.
- `src/network/server.rs:180-189` — `CatchupMailbox::replace` returns `false`
  when full and the coordinate is not already present.
- `src/network/server.rs:634-652` — `handle_host_command` ignores that boolean.
- `src/network/server.rs:191-203` — mailbox drain sorts by `(cx, cz)`, replacing
  the intended distance priority.

## Replay

1. Host a seed-42 world and mutate more than 32 loaded Chunks.
2. Join with a client whose socket writer is paused/throttled so the catch-up
   mailbox remains full.
3. Let `process_join_catchups` submit at least 33 distinct coordinates.
4. On the 33rd coordinate, observe `pending.pop_front()` on the main thread,
   followed by `CatchupMailbox::replace(...) == false`.
5. Resume the client and wait until all remaining host queues report empty.
6. Compare host/client Chunk checksums, or inspect the rejected coordinate.

Expected: backpressure leaves the coordinate owned by a retryable host/server
queue until explicit acceptance; all Chunk revisions eventually arrive in
distance order.

Actual: the coordinate has been removed from the host deque, rejected by the
mailbox, and is not retried. The client can permanently miss that Chunk; drain
order is coordinate order rather than distance order.

## Closure gap

There is no capacity=1, slow-client, multi-client, unloaded-mutated-Chunk
checksum test, and no cross-channel snapshot/BlockChange revision rule. R3 must
add explicit acceptance/retry ownership, observable drop/retry counters, and
eventual host/client checksum convergence.

