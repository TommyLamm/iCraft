# Plan 17 manual QA checklist

This checklist records evidence that cannot be produced by the headless test
suite. Do not mark a row complete without a dated run, platform, and artifact.

## Resource packs and localization

- [ ] Start from a clean checkout with no `resourcepacks/` directory; launch
  the menu and create a world. Confirm built-in textures, sounds, and English
  text load without an external path.
- [ ] Add a valid directory pack and a valid ZIP containing `pack.json`, one
  texture, one sound, and `lang/de_de.json`. In Resource Packs, verify list
  order, enable/disable, Apply, Reload, and German fallback behavior.
- [ ] Try ZIP entries `../escape`, an absolute/drive path, a symlink, a file
  over 8 MiB, an archive over 128 MiB, and a high-ratio deflate entry. Confirm
  the pack is rejected, no file is extracted, and a missing-asset diagnostic is
  emitted once rather than once per frame.
- [ ] Switch English ↔ German in the menu and HUD while chat is open. Confirm
  focus, input text, item/block names, death, command, disconnect, and
  advancement strings remain usable; restart once to verify persistence.

## Accessibility and input

- [ ] Navigate every menu screen using Tab, Shift+Tab, Enter, and Escape only.
  Confirm the gold focus ring is visible, wraps in a stable order, and text
  fields retain their input while moving between screens. Verify mouse clicks
  still activate the same controls.
- [ ] Exercise every Accessibility row: UI scale 0.75–2.0, chat scale and
  opacity, subtitles, high contrast, reduce flashing, toggle sprint/sneak,
  camera bobbing, and damage tilt. Confirm values survive a restart.
- [ ] At 4:3 (1024×768), 16:9 (1920×1080), 21:9 (3440×1440), and a Windows
  high-DPI scale, inspect menu buttons, chat, subtitle queue, death screen,
  and critical controls for clipping or overlap.
- [ ] Trigger jump, hurt, block, thunder, and explosion sounds from several
  directions. With subtitles on, verify direction labels, bounded queue, and
  expiry. With reduce flashing on, verify only lightning/End/damage visuals
  are dimmed; tick rate and damage values remain unchanged.

## Runtime, persistence, and performance

- [ ] Run the singleplayer foundation/progression/social scenarios from
  `cargo test --release --lib final_acceptance`; retain the test log.
- [ ] Run a fixed-view GPU scene for at least 30 minutes with the selected pack,
  subtitles, chat, and accessibility toggles. Record FPS, frame time, memory,
  and any device loss; compare with the approved performance artifact.
- [ ] Save and quit after resource-pack, language, accessibility, and gameplay
  changes; restart and verify world, player, settings, and selected packs.
- [ ] For listen-server and dedicated server + two clients, execute the same
  three scenarios and assertions. These rows are blocked until Plan 18's
  authority-unification follow-up is complete; attach the logs when rerun.
