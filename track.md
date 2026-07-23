# Current Work Track

> Last updated: 2026-07-24
> Goal: complete tasks 1-9, then audit, list, and fix latent bugs as task 10.
> Rule: complete, verify, and commit each task separately.

## Persistent checklist

| # | Task | Status | Commit | Verification |
|---|---|---|---|---|
| 1 | [Complete render optimization](plans/implementation/01_render_optimization.md) | Complete | `768c590` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (182 unit + 1 integration); WGSL validation |
| 2 | [Smooth remote-player movement](plans/implementation/02_multiplayer_smoothing.md) | Complete | `2c72b82` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (191 unit + 1 integration); targeted interpolation, protocol, relay, latest-wins, transport, and velocity tests |
| 3 | [Add Minecraft-style Creative flight](plans/implementation/03_creative_flight.md) | Complete | `b6dcf9b` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (201 unit + 1 integration); 10 flight/input/physics regressions |
| 4 | [Reject placement intersecting a player](plans/implementation/04_player_placement_collision.md) | Complete | `b8aaaf6` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (210 unit + 1 integration); placement AABB, latest authoritative pose, event classification, and authenticated-session regressions |
| 5 | [Add a proper 3D torch model](plans/implementation/05_torch_model.md) | Complete | `0ea9c8d` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (214 unit + 1 integration); exact bounds/count, UV, winding, AO/light, properties, support/light cleanup |
| 6 | [Fix Survival attacks against mobs](plans/implementation/06_survival_combat.md) | Complete | `f9930d1` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (219 unit + 1 integration); hit/miss/latch routing, target filtering, invulnerability/impact, and zero-HP cleanup |
| 7 | [Add adjustable weather/rain volume](plans/implementation/07_weather_volume.md) | Complete | `7fa3b6b` | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (226 unit + 1 integration); legacy/clamp/roundtrip settings, category gain, live-loop refresh, and UI hit regions |
| 8 | [Add a Creative item catalog on `E`](plans/implementation/08_creative_inventory.md) | Complete | `f5b69f8` | `cargo fmt --all -- --check`; `cargo check --release`; `cargo test --release` (238 unit + 1 integration); exact catalog/partition, virtual supply/no-op, hotbar/cursor safety, wheel routing, SplashPotion metadata, and multi-aspect layout regressions |
| 9 | [Stop camera rotation while inventory is open](plans/implementation/09_inventory_camera_lock.md) | Complete | pending commit | `cargo fmt --all -- --check`; `cargo check --release`; `cargo test --release` (243 unit + 1 integration); seven-blocker predicate, disabled/enabled mouse deltas, sensitivity/pitch clamp, UI NDC, E repeat, and Creative wheel regression |
| 10 | Last: find, list, and fix latent bugs | Pending | — | — |

## Working notes

- The repository was clean on `master` at `ac8f57e` before this work began.
- Sub-agents implement each task without committing. The root agent reviews
  their changes, requests corrections when needed, updates documentation, runs
  final verification, and creates one isolated commit per task.
- Task 10 must run only after tasks 1-9 are complete, per the user's ordering.
- Task 1 implements conservative greedy terrain meshing, owned halo snapshots
  and bounded Rayon chunk load/remesh jobs, generation/lifetime/revision stale
  result rejection, actual-bounds frustum culling, sorted opaque/translucent
  draw plans, three terrain LODs, a render-distance-aware far plane, and
  submitted-geometry F3 statistics. The hardware-dependent Render Distance 16
  `60+ FPS` observation remains an explicit manual check in the task document.
- Task 2 fixes the stop/start root cause by replacing the two-point pose state
  with a bounded timestamped snapshot history. It adds protocol-v3
  sequence/timestamps, real target bracketing, shortest-yaw interpolation,
  100 ms bounded extrapolation, teleport snap, invalid/out-of-order rejection,
  retained animation velocity, TCP no-delay/single-write framing, and
  latest-wins pose delivery without weakening reliable world/chat traffic.
  The interactive Host + Join visual check remains explicitly manual.
- Task 3 adds non-repeat 300 ms Jump double-tap flight toggling, horizontal
  camera-yaw movement, Space/Shift ascent/descent, sprint flight, hover, solid
  collision, ceiling/landing handling, and fall-distance resets. Flight and
  flight velocity are transient; Survival switching, death, respawn and
  dimension travel exit safely, while UI/focus changes clear pending taps but
  preserve active hover. F3 identifies the flying state. Interactive camera
  and multiplayer visual checks remain manual.
- Task 4 rejects any solid placement with positive-volume overlap against the
  current local player AABB or the latest authoritative snapshot of any remote
  player. Face/edge/corner contact and non-solid blocks remain legal. Joined
  clients preflight before sending, while the Host preserves the authenticated
  session ID and repeats the final check before world mutation or broadcast.
  Interactive single-player and Host + Join placement checks remain manual.
- Task 5 gives ground torches a dedicated 2x2x10-pixel six-face cuboid mesh
  instead of the generic full-block cube. Face-specific inset UVs stay within
  atlas tile `(4,2)`, all faces keep source-cell light and AO 1.0 without
  directional shading, and existing cutout/light/support/non-solid behavior is
  unchanged. Interactive visual inspection from multiple angles remains manual.
- Task 6 routes every left-button press through authoritative melee targeting
  before mode-specific block interaction. Survival misses retain held mining,
  while hits (including invulnerability-window interception) suppress mining
  behind the target; Creative misses alone use instant break. Only living
  combat entities can intercept the ray, and zero-HP ordinary mobs now leave
  the entity list without disrupting nonliving or boss-owned lifecycles.
  Joined clients remain block-authority-only because mob state replication is
  not present. Interactive weapon/empty-hand combat remains manual.
- Task 7 adds a persistent Weather volume category with a quieter 40% default.
  Rain and Thunder use `Master x Sound x Weather`; ordinary SFX remain on
  `Master x Sound`. Master/Weather changes immediately refresh active loops,
  and both main-menu Options and the pause menu edit the same settings source.
  Interactive rainy-weather adjustment and restart persistence remain manual.
- Task 8 replaces Creative's incomplete prefilled inventory view with a virtual
  infinite catalog containing all 144 non-Air items exactly once. Seven tabs,
  a row-scrolled 9x5 window, adaptive scrollbar, and the nine real hotbar slots
  share one layout; Survival and every station retain the standard inventory.
  Catalog clicks create max-stack/one-item cursor stacks without mutating
  storage, while cursor-origin tracking discards only catalog-created stacks
  and losslessly returns real hotbar stacks. The catalog consumes inventory
  wheel input without changing the selected hotbar slot, and SplashPotion now
  starts with water+splash metadata. Interactive GPU/UI inspection remains
  manual.
- Task 9 routes raw mouse motion through one seven-blocker camera-look
  predicate covering pause, inventory, advancements, chat, disconnect, death,
  and focus. CursorMoved still updates UI hover coordinates. Cursor grab and
  visibility changes are centralized, use Locked with a Confined fallback for
  pure gameplay, and release for every blocked state. Inventory/advancement
  transitions are mutually exclusive without intermediate re-grabs, E ignores
  key repeat, and focus/death/respawn use the same synchronization path.
  Windows cursor-mode fallback and event timing remain an interactive check.

## Commit discipline

Before every task commit:

1. Update this file with the task status, tests, and resulting commit hash
   (write `pending commit` before creating the commit, then record the hash in
   the following task's commit if necessary).
2. Run task-focused tests plus formatting and compilation checks appropriate to
   the changed surface.
3. Stage only files belonging to that task and inspect the staged diff.
