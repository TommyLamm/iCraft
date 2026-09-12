# Plan10 — `container_sessions` 與 `ServerWorld` 職責解耦

## 定位

在現行架構中，`src/container_sessions.rs` 存在職責混雜與重複發明的問題：
1. **重複發明 `double_chest_partner`**：
   - `src/block_entity.rs:L854-880` 定義了 `pub fn double_chest_partner(...)`（支援普通箱與 EndCity 箱）。
   - `src/container_sessions.rs:L134-163` 又自造了 30 行的 `get_double_chest_partner`（且漏判 EndCityChest）。
   應直接刪除 `container_sessions.rs` 的重複實現，全庫統一走 `block_entity::double_chest_partner`。

2. **`ServerWorld` 依然依賴 `ContainerSessionManager` 靜態方法**：
   - `ServerWorld` 透過 `ContainerSessionManager::get_container_slots` 與 `set_container_slots` 操作方塊實體容器。
   - 事實上 `get_container_slots` 與 `set_container_slots` 僅僅是操作 `ChunkManager` 底下的方塊實體，與 session 管理器狀態（`Vec<ContainerSession>`）完全無關。
   - 應將這兩個方法直接移至 `ChunkManager`（如 `ChunkManager::container_slots` / `set_container_slots`），使 `ServerWorld` 徹底解除對 `container_sessions.rs` 的引用。

3. **`simulate_container_click` 繞行包裝**：
   - `container_sessions.rs:L489-496` 的 `simulate_container_click` 僅是 `crate::inventory::apply_stack_click` 的 8 行薄包裝。
   - `authority/dispatch.rs` 可直接調用 `crate::inventory::apply_stack_click`。

預期削減代碼 ~160 行。

## 前置

06 已完成。

## 精確 acceptance

- [ ] 移除 `src/container_sessions.rs` 中的 `get_double_chest_partner`，改用 `block_entity::double_chest_partner`。
- [ ] 移除 `src/container_sessions.rs` 中的 `simulate_container_click`，調用端直連 `crate::inventory::apply_stack_click`。
- [ ] 將容器槽位讀寫功能遷移至 `ChunkManager`，`ServerWorld` 零依賴 `ContainerSessionManager`。
- [ ] 保持容器點擊、雙箱同步、開關箱邏輯的行為 100% 守恒。
- [ ] `cargo check --all-targets` 通過。
- [ ] 容器點擊守恒測試（`review_hardening_container_click.rs`）全數通過。

## 預計檔案與測試

- 修改：
  - `src/container_sessions.rs`
  - `src/chunk_manager.rs`
  - `src/server_world.rs`
  - `src/authority/dispatch.rs`
- 驗證測試：
  - `cargo test --test review_hardening_container_click -- --test-threads=1`
  - `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 將 `get_container_slots` 與 `set_container_slots` 搬遷至 `src/chunk_manager.rs`。
2. 更新 `ServerWorld` 與 `dispatch.rs` 的調用點。
3. 刪除 `container_sessions.rs` 中的重複與薄包裝函式。
4. 運行容器整合測試。

## 不在本計劃

- 修改容器介面點擊的物品堆疊分割規則（`apply_stack_click`）。
