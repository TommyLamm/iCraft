# Plan 17 known differences and hand-offs

## Foundation gaps

- Listen-server and dedicated + two-client acceptance is intentionally blocked
  by the authority-unification work tracked in Plan 18. The Plan 17 harness
  reports those topology rows as blocked rather than claiming a pass.
- The headless singleplayer scenarios execute gameplay workflows and save/reload
  seams, but do not construct a window, GPU or audio device. A separate Plan18
  harness starts a real dedicated runtime and two TCP clients and verifies
  authority save/restart; it does not yet execute all three Plan19 scenarios.
- Resource-pack application is selected at menu time and consumed when a world
  state is created. A live world does not hot-swap GPU atlas resources.
- Selected model descriptors now flow through the background main-world L0/L1/L2
  meshing entries, including greedy, non-full, special and portal geometry. The
  selected bitmap font reaches Menu plus State HUD/chat/death/inventory/debug/
  subtitle/boss/advancement text, with procedural/built-in fallback. A live world
  still does not hot-swap GPU atlas/model/font resources after pack order changes.

## Content differences

- The first pack resolver covers texture/item/block model descriptors, sounds,
  fonts, and languages through bounded logical asset lookup. It does not attempt
  the complete vanilla model graph, every sound variant, or every font feature.
- Procedural artwork remains the deterministic fallback for missing texture and
  sound bytes. This is an intentional compatibility fallback, not a claim of
  complete vanilla art parity.
- German (`de_de`) is complete for the Plan 17 required key set; other languages
  use English fallback and report missing keys in catalog diagnostics/tests.

## Explicitly unsupported

- Core shader overrides, Marketplace packages, Java mod loaders, and arbitrary
  third-party pack formats are outside Plan 17.
- GPU/window visual QA, audio-device behavior, high-DPI layout evidence,
  30-minute soak, and fixed-view performance thresholds require the manual QA
  checklist and are not marked complete by source tests alone.
