# iCraft

iCraft is an experimental voxel game written in Rust. It uses `wgpu` for
rendering and includes procedurally generated worlds, configurable controls,
and persistent world saves.

## Getting started

Install the latest stable [Rust toolchain](https://www.rust-lang.org/tools/install),
then run the game from the repository root:

```text
cargo run --release
```

For a faster development build, omit `--release`.

## Assets, resource packs, and languages

The repository ships a small, self-contained `assets/` pack with procedural
fallbacks. Optional user packs live in the workspace-relative `resourcepacks/`
directory and must contain a `pack.json` manifest. The menu's Resource Packs
screen validates dependencies, ordering, archive paths, and byte budgets before
applying a pack. Missing or invalid assets fall back to the built-in artwork
and emit a one-time diagnostic; shader overrides and mod/Marketplace formats
are intentionally outside this first implementation.

`assets/lang/en_us.json` is the fallback catalog and `de_de.json` is the bundled
second language. Settings persist language, UI/chat scale, subtitles, contrast,
reduced flashing, and input toggles in `settings.txt`.

`ICRAFT_RESOURCE_PACK` is an explicit development/test override for a single
pack location. It is never used as the default discovery path.
