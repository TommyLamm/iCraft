# Scenario 08: Multiplayer Host & Client Join Stress

> Seed: `1337008`
> Render Distance: 8
> Target: Multiplayer server/client queue depth, pose fan-out, block mutation broadcast

## Description

Tests network event processing, pose fan-out, and queue depth under local host + joining client setup.

## Setup & Execution

1. Start host instance on port 25565 with seed `1337008`.
2. Connect client instance to `127.0.0.1:25565`.
3. Move both players while placing/destroying blocks.
4. Record `network_queue_depth` on both host and client F3 overlays.

## Metrics to Track

- `network_queue_depth` counter
- `NetworkDrain` CPU scope average and p95
- Position fan-out latency
