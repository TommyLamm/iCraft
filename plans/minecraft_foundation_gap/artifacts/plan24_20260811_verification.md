# Plan24 verification artifact — 2026-08-11

All commands were run from `F:\Desktop\MC` on Windows x86_64. Temporary world
directories and the generated `server.properties` were removed after each run;
the dated soak log is retained beside this file.

## Automated gates

- `cargo fmt --all -- --check` — pass.
- `git diff --check` — pass (final check recorded after the source/doc edits).
- `cargo test --locked --no-fail-fast` — pass: 656 passed/3 ignored in the
  library target, 787 passed/3 ignored in the second full target, and all
  integration targets passed with zero failures.
- `cargo test --release --locked --no-fail-fast` — pass with the same target
  counts and zero failures.
- `cargo check --all-targets` — pass.
- `cargo check --release --locked` — pass.
- `cargo test --lib network::server::tests` — 35 passed, 0 failed.
- `cargo test --release --locked --quiet --lib
  network::server::tests::transport_metrics_count_exact_successful_tcp_frames`
  — 50/50 isolated runs passed.
- `cargo test --test runtime_topology_parity
  plan24_plan22_gameplay_vectors_match_all_runtime_topologies -- --nocapture`
  — pass: one test iterating Singleplayer/ListenServer/Dedicated, including
  owner-private workstation projections, duplicate/stale/out-of-order gates,
  death/respawn, dimension transfer, and named-session reconnect.

## Dedicated CLI evidence

- `cargo run --release --locked --bin icraft-server -- --config
  plans/minecraft_foundation_gap/artifacts/server.properties --world
  plans/minecraft_foundation_gap/artifacts/headless_once_world --bind 127.0.0.1
  --port <ephemeral> --once` — exit 0, metrics `ticks=1`, `saves=1`.
- The same command with `--ticks 100` — exit 0, metrics `ticks=100`,
  `queue_depth=0`, `queue_full=0`, `saves=1`.
- Wall-clock dedicated soak command (duration flag is bounded automation for
  the existing headless CLI):

  ```text
  target/release/icraft-server.exe --config plans/minecraft_foundation_gap/artifacts/server.properties --world plans/minecraft_foundation_gap/artifacts/headless_wall_soak_20260811_220121_world --bind 127.0.0.1 --port 56912 --duration-seconds 1800
  ```

  Started `2026-08-11T22:01:21.899+08:00`, exited 0 at
  `2026-08-11T22:31:22.016+08:00` (1800 seconds); PID 40284, PTY session
  14216. Retained log:
  `headless_wall_soak_20260811_220121.log`. Final line reports
  `ticks=35842`, `players_online=0`, `loaded_chunks=1`, `entities=0`,
  `queue_depth=0`, `queue_full=0`, `saves=6`, `last_tick_us=4`,
  `max_tick_us=28547`, `last_save_ms=26`, with no panic, error, or tick-stall
  output. Five-minute health samples recorded PID responsiveness and stable
  working set (11.4–12.6 MiB).

GPU/window/audio/DPI and real Host+Join visual evidence remain explicit manual
boundaries and are not claimed by this artifact.
