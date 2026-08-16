# Scenario 04: High-Density Redstone Circuit

> Seed: `1337004`
> Render Distance: 8
> Target: Redstone 20 Hz tick execution, signal propagation, piston actuation

## Description

Measures CPU execution cost of a high-frequency redstone clock powering repeaters, comparators, and pistons.

## Setup & Execution

1. Load world with seed `1337004`.
2. Construct/trigger a 2-tick redstone clock array (100 repeaters and torches).
3. Observe redstone CPU tick scope on F3 over 300 world ticks (15 seconds).

## Metrics to Track

- `Redstone` CPU scope average, p95, p99
- Frame time variance during active redstone ticks
