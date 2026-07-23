# Current Work Track

> Last updated: 2026-07-23
> Goal: finish render optimization and the six requested gameplay/network/audio fixes.
> Rule: complete, verify, and commit each task separately.

## Persistent checklist

| # | Task | Status | Commit | Verification |
|---|---|---|---|---|
| 1 | [Complete render optimization](plans/implementation/01_render_optimization.md) | Complete | pending commit | `cargo fmt -- --check`; `cargo check --release`; `cargo test --release` (182 unit + 1 integration); WGSL validation |
| 2 | [Smooth remote-player movement](plans/implementation/02_multiplayer_smoothing.md) | Pending | — | — |
| 3 | [Add Minecraft-style Creative flight](plans/implementation/03_creative_flight.md) | Pending | — | — |
| 4 | [Reject placement intersecting a player](plans/implementation/04_player_placement_collision.md) | Pending | — | — |
| 5 | [Add a proper 3D torch model](plans/implementation/05_torch_model.md) | Pending | — | — |
| 6 | [Fix Survival attacks against mobs](plans/implementation/06_survival_combat.md) | Pending | — | — |
| 7 | [Add adjustable weather/rain volume](plans/implementation/07_weather_volume.md) | Pending | — | — |
| 8 | [Add a Creative item catalog on `E`](plans/implementation/08_creative_inventory.md) | Pending | — | — |
| 9 | [Stop camera rotation while inventory is open](plans/implementation/09_inventory_camera_lock.md) | Pending | — | — |
| 10 | Last: find, list, and fix latent bugs | Pending | — | — |

## Working notes

- The repository was clean on `master` at `ac8f57e` before this work began.
- `ARCHITECTURE.md` says remote poses are sent at 20 Hz and rendered using two
  snapshots at a fixed 100 ms delay. This is the first suspected cause of the
  visible stop/start motion and teleporting.
- Sub-agents are investigating each task read-only. The root agent owns all
  edits, verification, and commits so commits remain isolated by task.
- Task 10 must run only after tasks 1-9 are complete, per the user's ordering.
- Task 1 implements conservative greedy terrain meshing, owned halo snapshots
  and bounded Rayon chunk load/remesh jobs, generation/lifetime/revision stale
  result rejection, actual-bounds frustum culling, sorted opaque/translucent
  draw plans, three terrain LODs, a render-distance-aware far plane, and
  submitted-geometry F3 statistics. The hardware-dependent Render Distance 16
  `60+ FPS` observation remains an explicit manual check in the task document.

## Commit discipline

Before every task commit:

1. Update this file with the task status, tests, and resulting commit hash
   (write `pending commit` before creating the commit, then record the hash in
   the following task's commit if necessary).
2. Run task-focused tests plus formatting and compilation checks appropriate to
   the changed surface.
3. Stage only files belonging to that task and inspect the staged diff.
