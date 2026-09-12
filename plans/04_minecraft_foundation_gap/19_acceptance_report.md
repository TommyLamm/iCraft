# Plan 19 automated acceptance report

## Run metadata

- Date: 2026-08-10
- Platform: Windows, x86_64, headless test runners
- Revision baseline: `83a852b` (includes Plan19 acceptance fix `a5a69ef`)
- Manual GPU/window/audio artifacts: not produced by this report

## Scenario matrix

| Topology | Scenario | Observable workflow evidence | Save/reload evidence | Result |
|---|---|---|---|---|
| Singleplayer headless | Foundation | 10 assertions: wood/tool crafting, mining durability/drop, furnace XP, chest access, farming, food, bed, death/drop recovery, checksum | chunk, block entity and inventory reconstruction | Pass |
| Singleplayer headless | Progression | 8 assertions: resource chain, generated 12-frame stronghold, Nether/Fortress/return, Eyes, End, Dragon, End City loot | End City container loot reload | Pass |
| Singleplayer headless | SocialAutomation | 4 assertions: villager trade conservation/cooldown, hopper→furnace, minecart mount/move, deterministic ticks | minecart/player state reload seam | Pass |
| Listen server | All three | Same scenario rows exist but are intentionally reported blocked | Not run | Blocked by Plan18 State mutation convergence |
| Dedicated + 2 TCP clients | Authority transport slice | login, block/chest open/click, duplicate/out-of-order/stale, interest/viewer isolation, disconnect save, restart/reconnect | block, slot, revision, position and dimension restore | Pass; this is not the full three-scenario suite |

Commands and results:

- `cargo test --locked --no-fail-fast`: lib 593 passed/3 ignored; game bin 714 passed/3 ignored; server bin 2 passed; persistence 3 passed; headless authority 1 passed; integration 1 passed.
- `cargo test --release --locked --no-fail-fast -- --format terse`: the same totals passed. One earlier full release attempt had a non-reproducible library failure whose name was truncated; an immediate library rerun and complete rerun both passed.
- `cargo check --release --locked`, `cargo fmt --all -- --check`, and `git diff --check`: passed.
- Focused Plan19 evidence: `final_acceptance` 2/2 and `sim_harness::tests` 5/5 passed in debug and release.

## Resource and accessibility evidence

- Resource safety and descriptor tests cover bounded directory/ZIP loading, traversal and archive metadata rejection, dependency ordering, diagnostics, selected locale fallback, texture/audio fallback, model registry seam and menu font source.
- Gameplay `State` uses the selected translation catalog for HUD/chat/death/commands/disconnect/advancement and item/block/entity names; menu live language and settings persistence have automated tests.
- Accessibility/audio/localization/menu/State targeted suites cover camera-relative subtitles, bounded/deduplicated expiry, reduced flashing helpers, camera bob/damage presentation, keyboard focus/scroll/text-field transitions, settings persistence, scale and common aspect bounds.

The model descriptor seam is not yet the universal main-world mesh path, the custom font source is not the universal HUD font, and no screenshot/audio-device/DPI artifact was captured. Therefore Plan19 B/C and manual QA completion checkboxes remain open.
