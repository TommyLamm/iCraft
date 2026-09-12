# Scenario 07: Autosave & Continuous Dirty Chunk Save

> Seed: `1337007`
> Render Distance: 10
> Target: Background SaveManager execution, save queue depth, region cache memory

## Description

Measures non-blocking background save performance while repeatedly mutating terrain across multiple chunks.

## Setup & Execution

1. Load world with seed `1337007`.
2. Perform block mutations in 16 adjacent chunks.
3. Trigger background save via autosave timer or world save.
4. Record `save_queue_depth` and `loaded_region_cache_bytes` on F3.

## Metrics to Track

- `save_queue_depth` peak and drain duration
- `loaded_region_cache_bytes` memory usage
- Frame stutter during background save write
