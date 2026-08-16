# Plan04 — 握手身份與 online-mode

## 定位

- Handshake 只有 `{protocol_version, username}`。`ServerProperties.online_mode` 會從
  `server.properties`／`--online-mode` 解析、持久化、印在啟動 log，**握手與 join 從不讀它**。
- Operator 是 `operators.contains(username.to_ascii_lowercase())`。`Command` 在
  `session.operator || cheats_enabled` 時放行（`/give`、`/gamemode`、`/tp`）。
- 登入唯一性用原始名稱的 case-insensitive 比對。存檔路徑把非 `[A-Za-z0-9_-]` 折成 `_`，
  故 `foo_bar` 與 `foo.bar` 可同時在線、共用 `players/foo_bar.dat`。
- Windows 保留 stem（`con`／`prn`／`aux`／`nul`／`com1`–`lpt9`）可讓 `players/nul.dat`
  寫進裝置。空名稱變成 `players/.dat`。
- 預設專用伺服器綁 `0.0.0.0:25565`。在有真實憑證之前，名字即帳號。

## 前置

無。13 會依賴本計劃的「淨化後身份」當唯一檔名。

## 精確 acceptance

- [x] 定義單一正規化身份：小寫、僅 `[a-z0-9_-]`、長度 1..=16（或現有 32 的嚴格子集）。
  原始名稱經淨化後若改變 → 握手拒絕，不建 session。
- [x] 登入重複、whitelist、operator、`players/<id>.dat` 全部使用**同一個**正規化 key。
  `foo.bar` 不得載入或覆寫 `foo_bar.dat`。
- [x] 拒絕 Windows 保留 stem（大小寫不敏感，含 `con.txt` 這類加點變體若會落到裝置名）。
- [x] `online-mode=true`：在實作挑戰／shared secret／密碼雜湊之前，**啟動失敗**並寫清楚
  錯誤（「尚未實作驗證，拒絕當憑證開關」）。不得再印 `online_mode=true` 卻放行任何名字。
- [x] `online-mode=false` 文件化為 LAN／離線：TCP 連線**不得**因名字匹配而獲得 operator。
  Operator 只來自主控台 `op` 之後、綁定正規化 key 的後續連線（仍無密碼；這是明確的 LAN 模式）。
  若選擇更嚴：TCP 上完全不授 op，只留主控台指令。選一種寫進 acceptance 測試，不要兩種並存。
- [x] 測試：`foo.bar` vs `foo_bar` 第二個握手失敗；`Alice/../Alice` 失敗；`CON` 失敗；
  `online-mode=true` 的 `ServerProperties::validate` 或 `icraft-server` 啟動回 Err。

## 預計檔案與測試

- 修改：`src/save.rs`（`dedicated_player_file_path`）、`src/network/server.rs`（handshake／
  重複名）、`src/server_runtime.rs`（`online_mode`、`login_session`、operators）、
  `src/bin/icraft-server.rs`（啟動時驗證）。
- 測試：擴 `tests/authority_persistence.rs`（現在只 assert `starts_with(players/)`）；
  加 `src/save.rs` 與 `src/network/server.rs` 單元測。

## 建議階段

1. 抽出 `fn normalize_player_identity(raw: &str) -> Result<String, IdentityError>`，
   單元測試表驅動（合法、折損、保留名、空、過長）。
2. Handshake 與存檔都改呼叫它。
3. `online-mode=true` fail-closed。
4. 收斂 operator 授予規則並用測試釘死。

## 不在本計劃

- 真正常開的線上驗證（密碼、token、TLS）。本計劃只停止「假裝有 online-mode」。
- Chat／pose DoS（12）、symlink 世界目錄（13）。

## 實作與證據

單一身份入口是 `save::normalize_player_identity`：lowercase ASCII、`[a-z0-9_-]`、
長度 1..=16。若 lowercasing 後仍需改寫（例如 `foo.bar`）、過長、空名、或 Windows
保留 stem（`con`／`prn`／`aux`／`nul`／`com1`–`com9`／`lpt1`–`lpt9`，含 `con.txt`
這類加點變體）→ `Err`，握手不建 session。Handshake、login 重複檢查、whitelist、
console `op`／`deop`、以及 `players/<id>.dat` 都呼叫同一個函式。

Operator 政策（已用測試釘死）：TCP／`login_session` 只在正規化身份**已經**在
`operators` 集合裡時授 op。該集合只由專用伺服器主控台 `op`（與既有 persist
file）寫入。Handshake 沒有其他授 op 規則。`cheats_enabled` 仍獨立放行指令。

`online-mode=true` 在 `ServerProperties::validate`（因此也在 `load`／`write`／
`ServerRuntime::new`／`icraft-server` 啟動）fail-closed，錯誤含
「尚未實作驗證，拒絕當憑證開關」。`online-mode=false` 寫入 `server.properties`
註解與 `--help`，標成 LAN／離線：名字即帳號，仍無密碼。

驗證：

```
cargo test --lib save -- --nocapture
# 49 passed (incl. normalize_player_identity_table,
# dedicated_player_files_use_normalized_identity_and_reject_colliding_names)

cargo test --lib network::server -- --nocapture
# 37 passed (incl. handshake_rejects_mutating_and_reserved_identities,
# mutating_username_does_not_share_identity_with_sanitized_form)

cargo test --test authority_persistence -- --nocapture
# 4 passed (incl. mutating_identities_cannot_join_or_share_player_files)

cargo test --lib server_runtime -- --nocapture
# online_mode_true_fails_closed_at_validate_and_startup ok
# login_grants_operator_only_from_console_op_set ok

cargo test --bin icraft-server -- --nocapture
# 2 passed

cargo check --bins
cargo check --all-targets
```
