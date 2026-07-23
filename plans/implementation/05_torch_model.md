# 實作計畫 05：火把立體模型

## 狀態

✅ 已完成（2026-07-24）

## 目標

把目前完整方塊六面貼圖的 Torch 改成中央細柱立體模型，保留 cutout、
14 級發光、支撐規則與無碰撞語義。

## 已確認根因

`Chunk::generate_mesh` 只有 cross-model 特例；Torch 因而走一般
`BLOCK_FACES` 的 1×1×1 cube，六面又都使用 atlas `(4,2)`。

## 實作步驟

1. 在 `world.rs` 增加 `append_torch_mesh` 特殊路徑並在普通 cube 前處理。
2. 生成 Minecraft 尺寸地面火把：X/Z `7/16..9/16`，Y `0..10/16`，
   六面向外正確 winding。
3. 四側使用柄/火焰長 UV，頂面使用火焰區，底面使用柄末端像素；所有 UV
   保留 atlas tile 內 half-texel inset。
4. 所有頂點 AO 1.0，以來源格 sky/block light 打包，不套一般 cube 面陰影。
5. 保留 `Cutout`、`is_solid=false`、`light_emission=14` 與地面支撐規則。
6. 現有資料沒有通用 block-facing state，因此本次只做地面火把；不假裝
   支援無法存檔/同步的壁掛狀態。
7. 更新架構與進度文件。

## 驗證

- [x] 單火把 24 vertices / 36 indices，bounds 精確符合 2×2×10 pixel。
- [x] side/top/bottom UV 不越出 `(4,2)`，winding 全部朝外。
- [x] AO/來源格光照正確且無面陰影；移除支撐會移除火把和光源。
- [x] Cutout、非 solid、14 級發光與地面支撐語義不回歸。
- [x] `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- [ ] 人工從各角度查看，確認不再是完整方塊且無透明六面殘影。

自動驗證結果：214 項單元測試與 1 項整合測試全部通過；唯一編譯
警告是既有未使用的 `hand_camera_buffer`。

## Commit

單一功能 commit：`fix(render): add proper torch model`
