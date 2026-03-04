# Changelog

## 0.3.2

### Added
- **Preferences overhaul** — 15 new settings across 4 reorganized tabs (General, Appearance, Network, Terminal). See [docs/preferences.md](docs/preferences.md).
  - **Date format** — choose ISO, EU, US, or relative timestamps
  - **Default view mode** — start panels in list, icon, or column view
  - **Row height** — compact, normal, or comfortable density
  - **Confirm before overwrite** — always, never, or ask on conflict
  - **Quick filter toggle** — disable keystroke filtering
  - **Image tooltip toggle** — disable hover previews
  - **S3 multipart tuning** — configurable upload threshold, part size, and concurrent parts
  - **SFTP timeouts** — configurable inactivity, keepalive, and operation timeouts
  - **Terminal customization** — font size (live update), scrollback lines, custom shell path
- **Progress throttling** — file copy/move operations emit ~20 progress updates/sec instead of one per file, reducing IPC overhead for many small files

## 0.3.1

### Added
- **Virtualized list scrolling** — smooth scrolling with large directories
- **Terminal for remote backends** — terminal works in SFTP and S3 panels

### Fixed
- Terminal rendering on remote panel switch

## 0.3.0

### Added
- **Status bar** — unified notifications replacing toast messages
- **CodeMirror editor** — replaced textarea with CodeMirror 6 with syntax highlighting and linting
- **Syntax highlighting** — highlight.js for Viewer and Preview pane
- **PDF preview** — inline PDF rendering in preview pane
- **Preview pane** — replaces inactive panel (Alt+P), with directory history
- **Clipboard operations** — Cmd+Shift+C/X/V for copy, cut, paste
- **Configurable columns** — choose which columns to display
- **Command palette** — Cmd+Shift+P to find and run any command
- **Undo** — Cmd+Z to undo delete (trash) and rename operations
- **Directory comparison** — Cmd+Shift+D to diff left and right panels
- **Context menu** — right-click for file operations
- **Batch rename** — find/replace, prefix/suffix, numbering, case transforms
- **Per-panel tabs** — Cmd+Alt+T for new tabs, Cmd+Alt+W to close

### Fixed
- Archive file viewing (F3/Enter/Cmd+3)
- Tab shortcuts on macOS
- Various accessibility warnings
