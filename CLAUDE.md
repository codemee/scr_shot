# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **產品名稱：ezshot**（系統匣圖示名稱、視窗標題前綴、README 均已更新）

## Build & Run

```powershell
cargo build                  # debug build (快，有符號)
cargo build --release        # release build (LTO, opt-level=3)
cargo check                  # 只做型別檢查，不產生執行檔（最快）
cargo run                    # debug 模式執行（因 windows_subsystem="windows" 無 console 輸出）

# 手動執行
.\target\debug\ezshot.exe
.\target\release\ezshot.exe
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
| `editor/tool.rs` | `Stroke` enum（Pen/Arrow/Rect/Text/Crop），各工具 GDI 繪製邏輯；`Stroke::translate()` 供裁切後座標平移 |
| `editor/window.rs` | 編輯器視窗：工具列、捲軸、滑鼠事件 → canvas |
| `output/clipboard.rs` | Win32 clipboard CF_DIB 寫入（不用 arboard，HBITMAP 支援不完整） |
| `output/file.rs` | BGRA → RGBA 轉換後用 image crate 存 PNG |
| `config.rs` | 儲存路徑、游標擷取開關、延遲秒數；設定寫入 `%APPDATA%\ezshot\` |

### 截圖流程（含延遲）

1. 快捷鍵觸發 → `AppEvent` 送入 channel
2. state_machine 從 `Arc<Mutex<Config>>` 讀取 delay/cursor 設定
3. overlay 視窗（若需框選/點選）**立刻出現**（不在前面加 delay）
4. 使用者選好標的後：delay N 秒 → overlay `ShowWindow(SW_HIDE)` + 80ms GDI 刷新 → BitBlt
5. `editor::open()` 在子執行緒跑 message loop；關閉時必送 `EditorSave` 或 `EditorCancelled`
6. 編輯器開啟後用 `HWND_TOPMOST → SetForegroundWindow → HWND_NOTOPMOST` 確保前景

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

### 編輯器工具列

按鈕用 `WINDOW_STYLE(0x0000000Bu32)`（`BS_OWNERDRAW`）建立，`WM_DRAWITEM` 中：
- `DRAWITEMSTRUCT` 在 `windows::Win32::UI::Controls`，**不在** `WindowsAndMessaging`
- 按下狀態：`dis.itemState.0 & 0x0001`（`ODS_FLAGS` newtype，須用 `.0`）
- 繪製：`RoundRect` 背景 + **GDI 圖示**（MoveToEx/LineTo/Polygon/Arc/Rectangle）
- 工具游標（`WM_SETCURSOR`）：畫布區 (y ≥ TOOLBAR_H) 才切換；`hovering_canvas` flag 在 `WM_MOUSEMOVE` 維護
- 按鈕點擊後須 `SetFocus(hwnd)` 才能恢復 ESC 等快捷鍵

### Tooltip（自製，非 Win32 API）

Win32 tooltip API (`TTF_SUBCLASS`) 在 `BS_OWNERDRAW` 按鈕 + 父視窗不接收 `WM_MOUSEMOVE` 的情況下不可靠。  
改用**每 100ms 輪詢**的做法：

```rust
// WM_TIMER id=3 (每 100ms)
WindowFromPoint(cursor_screen) → GetDlgCtrlID → 識別是哪個按鈕
```
停留同一按鈕 500ms 後顯示自製 popup（獨立 WNDCLASS，淺黃底 + DEFAULT_GUI_FONT 文字）。

**關鍵陷阱**：子視窗（按鈕）擋住父視窗的 `WM_MOUSEMOVE`，父視窗永遠收不到按鈕上方的滑鼠移動事件。

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

### 工具列沉浸式風格

`TOOLBAR_BG: u32 = 0x00_F0_F0_F0` 作為工具列背景色。
- WM_PAINT 開頭明確 `FillRect` 填工具列區域（WM_ERASEBKGND 回傳 1，不靠視窗背景刷自動填）
- 非作用工具按鈕背景設成 `COLORREF(TOOLBAR_BG)`，讓 RoundRect 隱形、只留圖示
- 作用中工具 / 按下 / 動作按鈕保留各自顏色
- **WM_MOUSEMOVE 的 `InvalidateRect` 只刷畫布區域**（`top=TOOLBAR_H`），避免繪圖時工具列閃爍

### 下拉式顏色選取面板

非模態下拉：`WS_POPUP | WS_BORDER`（無 `WS_CAPTION`），定位在 BTN_COLOR 正下方：

```rust
// 取得按鈕螢幕位置
let btn = GetDlgItem(owner, BTN_COLOR as i32)?;
GetWindowRect(btn, &mut btn_rc);
// 在按鈕正下方建立面板
CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, ..., WS_POPUP | WS_BORDER | WS_VISIBLE,
    btn_rc.left, btn_rc.bottom, win_w, win_h, ...)
// SetCapture 收所有滑鼠事件；WM_LBUTTONDOWN 在視窗外 → 取消關閉
SetCapture(drop);
```

**關鍵**：下拉面板 WM_LBUTTONDOWN 用 client coordinates 判斷是否在範圍內（cx<0、cy<0、cx≥win_w、cy≥win_h → 視窗外）。

### 儲存對話框（IFileSaveDialog）

編輯器執行緒須先 `CoInitializeEx(None, COINIT_APARTMENTTHREADED)` 才能使用 COM。  
`SHCreateItemFromParsingName` 的 pbc 參數型別需明確：`None::<&IBindCtx>`。  
`IFileDialog` 方法（`SetFileName`、`SetFolder`、`SetDefaultExtension`）可直接對 `IFileSaveDialog` 呼叫（COM 繼承）。

### 裁切工具與 Undo 系統

`Canvas::crop(r: RECT)` 直接修改 `self.base`，並對所有 `strokes` 呼叫 `Stroke::translate(-x, -y)` 調整座標。  
裁切前先把完整快照推入 `undo_ops: Vec<UndoOp>`（`UndoOp::Crop { base, width, height, strokes }`），可以復原。

### 馬賽克工具

`Canvas::apply_mosaic(r: RECT, block_size: i32)` 對選取矩形套用像素化效果：
- 以 `block_size`×`block_size`（預設 12px）為單位，計算每個方塊內所有像素的平均 BGR，再填回
- 直接修改 `self.base` 像素資料，不新增 Stroke
- 套用前推入 `UndoOp::Mosaic { base: ScreenBitmap }`（只需快照 base，寬高與 strokes 不變）

Undo 系統採 `UndoOp` enum：
- `UndoOp::Stroke` → `strokes.pop()`
- `UndoOp::Crop { snapshot }` → 還原整個 canvas 狀態

新增筆畫須呼叫 `canvas.push_stroke(stroke, color, thickness)`（同步維護 undo_ops），**不可**直接 push 到 `canvas.strokes`。

### 編輯視窗標題

`update_window_title(hwnd, state)` 在每次切換分頁、儲存、關閉分頁時呼叫，格式為 `ezshot-<tab.name>`。

### 多分頁編輯器

每次截圖開一個新分頁，視窗以持久方式存在（`WM_CLOSE` 只隱藏）。

- `WM_NEW_TAB = WM_APP+2`：app 傳 `Box<ScreenBitmap>` raw ptr，editor 建立新分頁
- `WM_FORCE_QUIT = WM_APP+3`：TrayQuit 時送出，editor 真正銷毀
- `WM_SHOW_EDITOR = WM_APP+4`：雙按系統匣圖示，帶視窗到前景
- Editor HWND 存於 `Arc<Mutex<Option<isize>>>`，共享給 app.rs state machine
- 標籤列：`CreateRoundRectRgn` + `IntersectClipRect` 裁切到標籤列範圍 → 上方圓角、平底
- Tooltip 使用 `WS_EX_LAYERED | WS_EX_NOACTIVATE`，`SetWindowPos` 加 `SWP_NOACTIVATE`：
  不觸發下方 WM_PAINT，不搶奪焦點（避免編輯視窗陰影消失）

### 防閃爍（進階）

`WM_PAINT` 雙條件：

```rust
if ps.rcPaint.top < CANVAS_Y  { /* 工具列＋標籤列 */ }
if ps.rcPaint.bottom > CANVAS_Y { /* Canvas::render */ }
```

所有只影響畫布的 `InvalidateRect` 使用 `Some(&RECT{top:CANVAS_Y,...})`，**不呼叫 `update_scrollbars`**（普通筆畫不改變畫布尺寸），避免 `SWP_FRAMECHANGED` 污染髒區域。

### 倒數計時（show_countdown）

`overlay::show_countdown(seconds, highlight: Option<RECT>)` 在執行緒中同步阻塞 N 秒。

```rust
// 無 message loop 情況下同步繪製的方式：
InvalidateRect(hwnd, None, false); // 必須先標記髒區域
UpdateWindow(hwnd);                // 才會觸發同步 WM_PAINT
```

**不呼叫 `InvalidateRect`，`UpdateWindow` 不會重繪**（視窗已被標記為乾淨）。

`highlight` 參數：倒數期間在全螢幕透明 overlay 上畫橘色框標示擷取區域，使用與 pick overlay 相同的 `UpdateLayeredWindow` 技術。`WS_EX_TRANSPARENT` 確保滑鼠事件穿透。

### 前景視窗

`SetForegroundWindow` 在跨執行緒且距 WM_HOTKEY 超過 Windows 時間窗口時靜默失敗。  
可靠做法：
```rust
SetWindowPos(hwnd, HWND_TOPMOST, ..., SWP_NOMOVE|SWP_NOSIZE|SWP_SHOWWINDOW);
SetForegroundWindow(hwnd);
SetWindowPos(hwnd, HWND_NOTOPMOST, ..., SWP_NOMOVE|SWP_NOSIZE);
```

### Pick Overlay：alpha=1 取代 alpha=0

透明孔用 `0x01_00_00_00`（alpha=1）而非 `0x00_00_00_00`（alpha=0）。原因：`UpdateLayeredWindow` per-pixel alpha 下 alpha=0 像素**點擊穿透**到底下視窗，即使有 `SetCapture` 也可能遺失；alpha=1 視覺上透明但保留 hit-testing。

### windows crate 注意事項

- `SetScrollInfo`、`DRAWITEMSTRUCT` 在 `Win32_UI_Controls`，**不在** `Win32_UI_WindowsAndMessaging`
- `SetFocus`、`GetCapture`、`SetCapture`、`VK_*` 在 `Win32_UI_Input_KeyboardAndMouse`，不在 glob
- `ScreenToClient`、`ClientToScreen` 不在 `WindowsAndMessaging` glob；改用 `GetMessagePos()` + `GetWindowRect()` 計算
- `CheckMenuRadioItem`、`CheckMenuItem` 的 flags 參數型別是 `u32`，非 `MENU_ITEM_FLAGS`（須用 `.0`）
- `TOOLINFOW` 在 windows-rs 0.58 中名為 `TTTOOLINFOW`
- `WM_HSCROLL`/`WM_VSCROLL` 的 code（SB_LINEUP 等）型別是 `SCROLLBAR_COMMAND`，match 時須用 `.0` 或整數字面值
- `HWND` 不實作 `Send`，跨執行緒傳遞視窗 handle 須先轉成 `isize`
- `PtInRect` 在 `*` glob 不可用，改用直接座標比較
- `DefWindowProcW` 是泛型函式，不能直接作為 `WNDCLASSEXW.lpfnWndProc` 的函式指標；需包一層 `unsafe extern "system" fn wrapper(h, m, w, l) -> LRESULT { DefWindowProcW(h, m, w, l) }`
- 重新執行前先確認舊程序已結束：`Stop-Process -Name ezshot -Force`（否則 binary 被鎖定，`cargo build` 會失敗）
