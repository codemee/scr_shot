# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

```powershell
cargo build                  # debug build (快，有符號)
cargo build --release        # release build (LTO, opt-level=3)
cargo check                  # 只做型別檢查，不產生執行檔（最快）
cargo run                    # debug 模式執行（因 windows_subsystem="windows" 無 console 輸出）

# 手動執行
.\target\debug\srcshot.exe
.\target\release\srcshot.exe
```

> 沒有測試套件。功能驗證靠手動執行。

## 架構概覽

### 執行緒模型

```
主執行緒（Win32 message loop）
  ├─ tray::msg_wnd_proc     → 處理 WM_HOTKEY、WM_COMMAND（系統匣選單）
  └─ 把事件塞進 mpsc channel

state_machine 執行緒（消費 channel）
  ├─ AppState 狀態機驅動整個流程
  └─ 各動作 spawn 新執行緒

spawn 的執行緒（短暫存活）
  ├─ overlay::show_region() / show_pick()  → 各有自己的 Win32 message loop
  └─ editor::open()                        → 有自己的 Win32 message loop
```

所有 Win32 視窗必須在建立它的執行緒上使用（HWND 不是 Send）。`AppEvent::WindowPicked` 改存 `isize`（HWND.0）以繞過 Send 限制。

### 狀態機（app.rs）

```
Idle
  ├─(Alt+Shift+R)→ OverlayRegion → RegionSelected → Editing
  ├─(Alt+Shift+A)→                                   Editing
  └─(Alt+Shift+W)→ OverlayPick  → WindowPicked    → Editing
                                   OverlayCancelled → Idle
Editing
  ├─ EditorSave / EditorCancelled → Idle
  └─ X 按鈕關閉：WM_DESTROY 補送 EditorCancelled，確保回到 Idle
```

非法轉換（如 Editing 狀態收到 CaptureRegion）直接 `_ => {}` 忽略。

### 模組職責

| 模組 | 職責 |
|------|------|
| `event.rs` | `AppEvent` enum，跨執行緒傳遞的唯一溝通介面 |
| `app.rs` | 狀態機 + 主 Win32 message loop |
| `tray.rs` | Shell_NotifyIcon、右鍵選單；WM_HOTKEY 轉發給 hotkey 模組 |
| `hotkey.rs` | RegisterHotKey/UnregisterHotKey，三組快捷鍵 |
| `capture/screen.rs` | GDI BitBlt 截圖；active_window_rect()、window_rect(HWND) |
| `capture/overlay.rs` | 兩種 overlay 視窗（框選/點選），各自有內部 message loop |
| `editor/canvas.rs` | ScreenBitmap + 標註疊加，`flatten_to_bitmap()` 輸出最終影像 |
| `editor/tool.rs` | `Stroke` enum（Pen/Arrow/Rect/Text），各工具 GDI 繪製邏輯 |
| `editor/window.rs` | 編輯器視窗：工具列、捲軸、滑鼠事件 → canvas |
| `output/clipboard.rs` | Win32 clipboard CF_DIB 寫入（不用 arboard，HBITMAP 支援不完整） |
| `output/file.rs` | BGRA → RGBA 轉換後用 image crate 存 PNG |
| `config.rs` | 儲存路徑（預設桌面），未來可擴充快捷鍵設定 |

### 截圖流程

1. 快捷鍵觸發 → `AppEvent` 送入 channel
2. state_machine spawn 執行緒
3. overlay 視窗（若需框選/點選）在子執行緒跑 message loop，完成後送 `RegionSelected` / `WindowPicked`
4. 區域截圖：overlay 先 `ShowWindow(SW_HIDE)` + 等 80ms 讓 GDI 刷新，再 BitBlt
5. `editor::open()` 在子執行緒跑 message loop；關閉時必送 `EditorSave` 或 `EditorCancelled`

### 防閃爍

編輯器 `WM_ERASEBKGND` 回傳 1（阻止背景擦除），所有 `InvalidateRect` 用 `bErase=false`，WM_PAINT 以雙緩衝（mem DC）一次 BitBlt。

### 圖示（icon.rs）

`icon.rs` 提供 `create_app_icon() -> HICON`，以 GDI 繪製相機圖示（32×32，藍底白機身）。  
系統匣（`tray.rs`）與編輯器標題列（`editor/window.rs`）共用此函式。呼叫方負責在結束後呼叫 `DestroyIcon`。

### Pick Overlay 實作（UpdateLayeredWindow）

`show_pick()` 使用 **per-pixel alpha** layered window（`WS_EX_LAYERED`），以 `UpdateLayeredWindow` + 32bpp DIB 繪製：

- 全螢幕覆蓋填 `0x88_00_00_00`（半透明黑色）
- hover 視窗範圍刷 `0x00_00_00_00`（完全透明，讓底下視窗可見）
- 透明孔四周畫 4px 橘色邊框 `0xFF_FF_A8_00`（premultiplied）

**不可**同時使用 `SetLayeredWindowAttributes`，兩種模式互斥。

`find_window_at(overlay, pt)` 用 `EnumWindows` 枚舉頂層視窗取代 `WindowFromPoint`，原因：`WindowFromPoint` 會傳回子視窗（按鈕、捲軸），導致高亮錯誤視窗。

### 自訂按鈕（BS_OWNERDRAW + WM_DRAWITEM）

編輯器按鈕用 `WINDOW_STYLE(0x0000000Bu32)`（`BS_OWNERDRAW`）建立，在 `WM_DRAWITEM` 中：
- `DRAWITEMSTRUCT` 在 `windows::Win32::UI::Controls`，**不在** `WindowsAndMessaging`
- 按下狀態：`dis.itemState.0 & 0x0001`（`itemState` 是 `ODS_FLAGS` newtype，須用 `.0`）
- 繪製：`RoundRect` 背景 + `DrawTextW` 置中文字 + `DEFAULT_GUI_FONT`

### 巢狀 Dialog 不能呼叫 PostQuitMessage

`simple_input_dialog` 的 `WM_DESTROY` **絕不可**呼叫 `PostQuitMessage(0)`。  
原因：編輯器 `GetMessageW` 迴圈會消費這個 WM_QUIT，導致編輯器直接退出。  
正確做法：`WM_COMMAND` 設 `state.done = true` → `DestroyWindow(hwnd)`，`WM_DESTROY` 只回傳 `LRESULT(0)`。

### 捲軸顯示控制

`ShowScrollBar` 在 windows crate 中不可用。改用：
```rust
SetWindowLongW(hwnd, GWL_STYLE, style | WS_HSCROLL.0 as i32);
SetWindowPos(hwnd, None, 0,0,0,0, SWP_NOMOVE|SWP_NOSIZE|SWP_NOZORDER|SWP_FRAMECHANGED);
```

### windows crate 注意事項

- `SetScrollInfo`、`DRAWITEMSTRUCT` 在 `Win32_UI_Controls`，**不在** `Win32_UI_WindowsAndMessaging`
- `WM_HSCROLL`/`WM_VSCROLL` 的 code（SB_LINEUP 等）型別是 `SCROLLBAR_COMMAND`，match 時須用 `.0` 或整數字面值
- `HWND` 不實作 `Send`，跨執行緒傳遞視窗 handle 須先轉成 `isize`
- `PtInRect` 在 `*` glob 不可用，改用直接座標比較：`pt.x >= rc.left && pt.x < rc.right && pt.y >= rc.top && pt.y < rc.bottom`
- 重新執行前先確認舊程序已結束：`Stop-Process -Name srcshot -Force`（否則 binary 被鎖定，`cargo build` 會失敗）
