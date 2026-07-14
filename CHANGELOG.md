# Changelog

## [0.3.11] - 2026-06-21

### Added

- Add eject button to sidebar devices
- Add in-app SMB/NFS network share mounting

### Dependencies

- 48 dependency update(s) via Renovate

## [0.3.10] - 2026-05-23

### Added

- Add frontend CI workflow

### Fixed

- Fix existing svelte eslint errors

### Changed

- Migrate from keyring v3 to keyring-core + per-platform stores

### Dependencies

- 29 dependency update(s) via Renovate

### Other

- Adapt SFTP agent auth to russh 0.61 AgentIdentity enum
- Adapt s3::crypto to rand 0.10 API
- Switch from deprecated md5::Context::compute to finalize
- Enable crypto-rust and vendored features on dbus-secret-service-keyring-store
- Surface remote-connection failures instead of swallowing them

## [0.3.9] - 2026-03-27

### Fixed

- Fix missing agent_socket param in test helper
- Fix missing agent_socket param in MCP server

### Other

- Custom SSH agent socket path, dependency updates

## [0.3.8] - 2026-03-18

### Other

- Large directory performance (1M+ files)

## [0.3.7] - 2026-03-18

### Fixed

- Fix paste in S3 dialog, disable autocomplete in input fields

## [0.3.6] - 2026-03-17

### Fixed

- Fix package-lock.json out of sync with package.json

### Other

- Viewer search mode, Nix support, dependency updates

## [0.3.5] - 2026-03-15

### Changed

- Update README: macOS only (remove Linux references)

### Other

- Remove .beads directory from repo
- Disk Usage two-way sync, deep caching, and dependency updates

## [0.3.4] - 2026-03-07

### Added

- Add drag-and-drop onto directory rows, fix data loss and DnD issues

### Fixed

- Fix package-lock.json sync for CI
- Fix release CI: create release before uploading MCP assets

### Other

- Open archives from SFTP/S3, add download size prompt and overwrite shortcut
- Remove Linux build from release CI (macOS only for now)

## [0.3.3] - 2026-03-04

### Added

- Add model inspector enhancements: VRAM estimation, comparison, tensor visualization
- Add MCP server for S3 and SFTP operations
- Add model inspector command and frontend wiring
- Add MCP server documentation and link from README
- Add disk usage analyzer and bump version to 0.3.3
- Add --help/--version to furman-mcp and Homebrew formula

### Fixed

- Fix release CI: build furman-mcp binary before bundling
- Fix release CI: enable mcp feature for furman-mcp build

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
