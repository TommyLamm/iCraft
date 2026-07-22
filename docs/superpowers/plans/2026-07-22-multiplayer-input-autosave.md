# Multiplayer Input Autosave Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automatically save and restore the last entered multiplayer settings (host port, server address, join port, player username) across game sessions.

**Architecture:** Extend `GameSettings` struct and serialization in `src/menu.rs` with 4 multiplayer fields (`mp_host_port`, `mp_server_address`, `mp_join_port`, `mp_username`). Initialize `Menu` fields with `GameSettings`, and sync & save `GameSettings` whenever the user confirms connection (NEXT) or exits the multiplayer screen (BACK / ESC).

**Tech Stack:** Rust (wgpu, winit, std::fs).

## Global Constraints

- Preserve default values when `settings.txt` is missing or keys are omitted (`"25565"`, `"127.0.0.1"`, `"25565"`, `"PLAYER"`).
- All edits take place in `src/menu.rs`.
- Standard Rust testing (`cargo test`).

---

### Task 1: Extend `GameSettings` and add serialization unit tests

**Files:**
- Modify: `src/menu.rs:110-240`
- Test: `src/menu.rs` (inline unit tests at end of file)

**Interfaces:**
- Consumes: `GameSettings` load/save pipeline.
- Produces: `GameSettings` with fields `mp_host_port`, `mp_server_address`, `mp_join_port`, `mp_username`.

- [ ] **Step 1: Write failing unit test for `GameSettings` multiplayer persistence**

Add a test at the end of `src/menu.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_settings_multiplayer_serialization() {
        let mut settings = GameSettings::default();
        assert_eq!(settings.mp_host_port, "25565");
        assert_eq!(settings.mp_server_address, "127.0.0.1");
        assert_eq!(settings.mp_join_port, "25565");
        assert_eq!(settings.mp_username, "PLAYER");

        settings.mp_host_port = "25570".to_string();
        settings.mp_server_address = "192.168.1.50".to_string();
        settings.mp_join_port = "25571".to_string();
        settings.mp_username = "ALICE".to_string();

        // Save & load verify logic...
    }
}
```

- [ ] **Step 2: Run test to verify compilation failure**

Run: `cargo test test_game_settings_multiplayer_serialization`
Expected: FAIL due to missing fields `mp_host_port`, etc.

- [ ] **Step 3: Modify `GameSettings` to include multiplayer fields, `load`, and `save`**

In `src/menu.rs`:
1. Add fields to `GameSettings`:
```rust
pub struct GameSettings {
    // ... existing fields
    pub mp_host_port: String,
    pub mp_server_address: String,
    pub mp_join_port: String,
    pub mp_username: String,
}
```
2. Update `impl Default for GameSettings`:
```rust
    mp_host_port: "25565".to_string(),
    mp_server_address: "127.0.0.1".to_string(),
    mp_join_port: "25565".to_string(),
    mp_username: "PLAYER".to_string(),
```
3. Update `GameSettings::load`:
```rust
    "mp_host_port" => settings.mp_host_port = value.to_string(),
    "mp_server_address" => settings.mp_server_address = value.to_string(),
    "mp_join_port" => settings.mp_join_port = value.to_string(),
    "mp_username" => settings.mp_username = value.to_string(),
```
4. Update `GameSettings::save`:
Write `mp_host_port:{}\nmp_server_address:{}\nmp_join_port:{}\nmp_username:{}\n` entries.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/menu.rs
git commit -m "feat: add multiplayer field persistence to GameSettings"
```

---

### Task 2: Integrate `Menu` state with `GameSettings` and auto-save on navigation

**Files:**
- Modify: `src/menu.rs:880-1215`

**Interfaces:**
- Consumes: `GameSettings` with multiplayer fields.
- Produces: `Menu` that initializes with stored multiplayer values and auto-saves them on NEXT and BACK.

- [ ] **Step 1: Write test for `sync_and_save_multiplayer_settings` in `src/menu.rs`**

Add unit test verifying that `sync_and_save_multiplayer_settings` copies `Menu`'s input fields into `settings` and triggers save.

- [ ] **Step 2: Implement `sync_and_save_multiplayer_settings` and update `Menu::new`, `handle_click`, `back`**

1. In `Menu::new`:
Initialize `host_port: settings.mp_host_port.clone()`, `server_address: settings.mp_server_address.clone()`, `join_port: settings.mp_join_port.clone()`, `username: settings.mp_username.clone()`.

2. Add method `sync_and_save_multiplayer_settings(&mut self)` to `Menu`:
```rust
fn sync_and_save_multiplayer_settings(&mut self) {
    self.settings.mp_host_port = self.host_port.clone();
    self.settings.mp_server_address = self.server_address.clone();
    self.settings.mp_join_port = self.join_port.clone();
    self.settings.mp_username = self.username.clone();
    self.settings.save();
}
```

3. Call `self.sync_and_save_multiplayer_settings()` in `handle_click()` when successfully confirming `MenuScreen::Multiplayer` NEXT button before switching to `MenuScreen::Worlds`.

4. Call `self.sync_and_save_multiplayer_settings()` in `back()` when exiting `MenuScreen::Multiplayer`.

- [ ] **Step 3: Run unit tests and `cargo check`**

Run: `cargo test`
Run: `cargo check`
Expected: ALL PASS with 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/menu.rs
git commit -m "feat: auto-save multiplayer inputs on NEXT and BACK actions in menu"
```
