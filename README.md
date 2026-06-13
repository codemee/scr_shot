# ezshot

**Languages:** [繁體中文](README.zh-TW.md) | English

A Windows screenshot tool that lives in the system tray, with global hotkeys, annotation, and cropping.

## Features

### Capture Modes

| Hotkey | Mode |
|--------|------|
| `Alt+Shift+R` | Drag to select a region |
| `Alt+Shift+F` | Capture the full virtual screen |
| `Alt+Shift+A` | Capture the active window |
| `Alt+Shift+W` | Click a window or control to capture it |

Window captures use the visible DWM frame bounds, so invisible resize borders are not included.

### Delayed Capture

Set a delay (0 / 1 / 2 / 3 / 5 s or custom) from the tray menu to capture hover menus, tooltips, and other transient states. A countdown and an orange highlight box are shown during the delay.

### Editor

Each capture opens in a new tab in the persistent editor window:

| Tool | Description |
|------|-------------|
| Pen | Freehand drawing |
| Arrow | Draw arrows |
| Rectangle | Draw rectangles |
| Text | Click to type text |
| Crop | Drag to crop immediately (undoable) |
| Mosaic | Drag to pixelate a region (undoable) |
| Color / Thickness | Dropdown panel: 12 preset colors + system color picker; thickness input with line preview |
| Copy | Copy to clipboard |
| Save | First save opens a dialog (default filename is a timestamp); subsequent saves overwrite |
| Save As | Save to a new location |
| Undo | Undo the last stroke, crop, or mosaic |
| ≡ Settings | Open settings (shared with tray menu) |

#### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Alt+P` | Pen |
| `Alt+A` | Arrow |
| `Alt+R` | Rectangle |
| `Alt+T` | Text |
| `Alt+C` | Crop |
| `Alt+M` | Mosaic |
| `Ctrl+Z` | Undo |
| `Ctrl+C` | Copy to clipboard |
| `Ctrl+S` | Save |
| `Ctrl+Shift+S` | Save As |

- Each capture opens a new **tab** named with its timestamp (`YYYYMMDDhhmmss`)
- Window title shows `ezshot-<tab name>`
- Tabs with unsaved changes show a red dot; it clears on save
- Click × on a tab to close it — if unsaved, a flat confirmation dialog appears (Save / Don't Save / Cancel)
- Clicking the window × button prompts for each unsaved tab, clears all tabs, then hides the editor to the system tray
- Minimizing the window sends it to the taskbar and preserves all tabs
- The app exits only from the tray icon context menu
- Tabs scroll automatically when there are too many; the newest is always visible

### Settings (tray right-click / toolbar ≡ button)

- **Capture cursor**: include the mouse cursor in screenshots
- **Auto-copy to clipboard**: copy to clipboard immediately after capture, before opening the editor
- **Hide editor before capture**: hide the editor window while taking a screenshot
- **Capture delay**: countdown in seconds (custom value supported)
- **Close All Tabs**: prompts for each unsaved tab (Save / Don't Save / Discard All / Cancel); hides the window when done
- Settings are saved to `%APPDATA%\ezshot\settings.ini`
- Last save directory is remembered in `%APPDATA%\ezshot\last_dir.txt`

## Requirements

- Windows 10 / 11 (x64)
- Rust toolchain (only needed to build from source)

## Build

```powershell
cargo build --release
```

The output is `target\release\ezshot.exe`. No installation required — just run it and it appears in the system tray.

## Tech

Pure Rust + Win32 API (`windows` crate 0.58). No .NET or third-party UI framework.
