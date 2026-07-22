# Multiplayer Input Autosave Design

Date: 2026-07-22

## Overview

Currently, in `iCraft`, the input fields in the Multiplayer menu (`host_port`, `server_address`, `join_port`, and `username`) are initialized with hardcoded defaults (`"25565"`, `"127.0.0.1"`, `"25565"`, `"PLAYER"`) upon launching the application. When a user changes these values, they are only kept in memory for the duration of the session and reset when the game restarts.

This design adds persistence for multiplayer input values within `settings.txt` via `GameSettings`, saving the user's latest inputs when they confirm host/join setup or leave the multiplayer screen.

## Goals

1. Automatically remember the last entered host port, server IP/address, join port, and username across game restarts.
2. Maintain backward compatibility with existing `settings.txt` files (falling back to sensible default values if keys are missing).
3. Ensure settings are saved when confirming a server launch/connect ("NEXT") and when leaving the multiplayer screen ("BACK" / ESC).

## Design Details

### 1. Data Model (`GameSettings` in `src/menu.rs`)

Add four new string fields to `GameSettings`:
- `pub mp_host_port: String` (default `"25565"`)
- `pub mp_server_address: String` (default `"127.0.0.1"`)
- `pub mp_join_port: String` (default `"25565"`)
- `pub mp_username: String` (default `"PLAYER"`)

#### Loading (`GameSettings::load`)
Read key-value pairs from `settings.txt`:
- `"mp_host_port"` -> `settings.mp_host_port`
- `"mp_server_address"` -> `settings.mp_server_address`
- `"mp_join_port"` -> `settings.mp_join_port`
- `"mp_username"` -> `settings.mp_username`

If any key is missing or empty, preserve the default value.

#### Saving (`GameSettings::save`)
Append the four multiplayer keys to `settings.txt`:
```text
mp_host_port:<value>
mp_server_address:<value>
mp_join_port:<value>
mp_username:<value>
```

### 2. Menu Integration (`src/menu.rs`)

#### Initialization (`Menu::new`)
Initialize `Menu` fields using `settings`:
- `host_port: settings.mp_host_port.clone()`
- `server_address: settings.mp_server_address.clone()`
- `join_port: settings.mp_join_port.clone()`
- `username: settings.mp_username.clone()`

#### Persistence Helper (`Menu::sync_and_save_multiplayer_settings`)
Define a helper method on `Menu`:
```rust
fn sync_and_save_multiplayer_settings(&mut self) {
    self.settings.mp_host_port = self.host_port.clone();
    self.settings.mp_server_address = self.server_address.clone();
    self.settings.mp_join_port = self.join_port.clone();
    self.settings.mp_username = self.username.clone();
    self.settings.save();
}
```

#### Trigger Conditions
Invoke `self.sync_and_save_multiplayer_settings()` in:
1. `Menu::handle_click()` when the user clicks the NEXT button on `MenuScreen::Multiplayer` before transitioning to `MenuScreen::Worlds`.
2. `Menu::back()` when exiting `MenuScreen::Multiplayer` to `MenuScreen::Main`.

## Verification Plan

### Unit Tests
- Add inline tests in `src/menu.rs` verifying `GameSettings` parsing and saving of `mp_host_port`, `mp_server_address`, `mp_join_port`, and `mp_username`.

### Manual Verification
1. Launch `cargo run`, navigate to Multiplayer menu.
2. Edit Host Port to `25570`, Server Address to `192.168.1.100`, Join Port to `25570`, Username to `TEST_PLAYER`.
3. Click NEXT (or BACK), exit game.
4. Inspect `settings.txt` to confirm the new values were written.
5. Re-launch `cargo run` and open Multiplayer menu to verify the fields populate with the updated values.
