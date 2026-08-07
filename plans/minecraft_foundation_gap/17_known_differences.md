# Plan 17 known differences and hand-offs

## Foundation gaps

- Listen-server and dedicated + two-client acceptance is intentionally blocked
  by the authority-unification work tracked in Plan 18. The Plan 17 harness
  reports those topology rows as blocked rather than claiming a pass.
- The headless scenario fixtures exercise deterministic world, progression, and
  social/automation seams; they do not construct a window, GPU, audio device,
  real save/restart cycle, or TCP session.
- Resource-pack application is selected at menu time and consumed when a world
  state is created. A live world does not hot-swap GPU atlas resources.

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
