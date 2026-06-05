# ezshot

**Languages:** [繁體中文](README.zh-TW.md) | English

A Windows screenshot tool that lives in the system tray, with global hotkeys, annotation, and cropping.

## Features

### Capture Modes

| Hotkey | Mode |
|--------|------|
| `Alt+Shift+R` | Drag to select a region |
| `Alt+Shift+A` | Capture the active window |
| `Alt+Shift+W` | Click any window to capture it |

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

- Each capture opens a new **tab** named with its timestamp (`YYYYMMDDhhmmss`)
- Window title shows `ezshot-<tab name>`
- Tabs with unsaved changes show a red dot; it clears on save
- Click × on a tab to close it; closing all tabs hides the window (does not destroy it)
- `Esc` or the window × hides the window — tabs are preserved; double-click the tray icon to restore
- Tabs scroll automatically when there are too many; the newest is always visible

### Settings (tray right-click / toolbar ≡ button)

- **Capture cursor**: include the mouse cursor in screenshots
- **Auto-copy to clipboard**: copy to clipboard immediately after capture, before opening the editor
- **Hide editor before capture**: hide the editor window while taking a screenshot
- **Capture delay**: countdown in seconds (custom value supported)
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
