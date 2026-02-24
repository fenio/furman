<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="src/lib/assets/furman-logo-light.svg">
    <source media="(prefers-color-scheme: light)" srcset="src/lib/assets/furman-logo-dark.svg">
    <img alt="Furman" src="src/lib/assets/furman-logo-dark.svg" width="400">
  </picture>
</p>

<p align="center">
  A dual-panel file manager for macOS inspired by Norton Commander and Midnight Commander.
</p>

<p align="center">
  <a href="https://github.com/fenio/furman/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/fenio/furman?style=flat-square"></a>
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square"></a>
</p>

---

## Features

- **Dual-panel navigation** with Tab to switch between panels
- **S3 support** — browse and transfer files to/from any S3-compatible storage (AWS, MinIO, Backblaze B2, etc.)
- **Integrated terminal** — bottom panel (Cmd+T), Quake-style drop-down (Cmd+\`), or in-pane mode (Cmd+Shift+T)
- **File viewer** (F3) — text with line numbers, image preview, hex dump
- **File editor** (F4) — built-in text editor with dirty-state tracking
- **Search** (Cmd+F) — search by file name or file content with streaming results
- **Archive browsing** — navigate inside zip, rar, and 7z archives as if they were directories
- **Quick filter** — type to filter the file list in real time
- **Drag and drop** — drag files between panels to copy (or Shift+drag to move)
- **Sidebar** — favorites, mounted devices, S3 connections, theme toggle
- **Dark / Light theme** — auto-detects OS preference, toggle with Cmd+Shift+L
- **Icon and list views** — switch between list and grid layouts with configurable icon sizes
- **File watcher** — panels auto-refresh when files change on disk

## Installation

### Homebrew

```sh
brew install fenio/tap/furman
```

### Download

Grab the latest `.dmg` from the [Releases](https://github.com/fenio/furman/releases/latest) page. Both Apple Silicon (ARM) and Intel builds are available.

> **Note:** Furman is not signed with an Apple Developer certificate. On first launch macOS Gatekeeper will block it. To allow it, go to **System Settings > Privacy & Security** and click **Open Anyway**, or run:
> ```sh
> xattr -cr /Applications/Furman.app
> ```

### Build from source

```sh
git clone https://github.com/fenio/furman.git
cd furman
npm install
npm run tauri build
```

The `.dmg` will be in `src-tauri/target/release/bundle/dmg/`.

## Tech Stack

| Layer    | Technology |
|----------|------------|
| Frontend | SvelteKit 5, TypeScript, Vite |
| Backend  | Rust, Tauri 2 |
| Terminal | xterm.js |
| S3       | aws-sdk-s3 (Rust) |
| Platform | macOS (Apple Silicon + Intel) |

## Keyboard Shortcuts

### File Operations

| Shortcut | F-key | Action |
|----------|-------|--------|
| Cmd+R | F2 | Rename |
| Cmd+3 | F3 | View file |
| Cmd+E | F4 | Edit file |
| Cmd+C | F5 | Copy to other panel |
| Cmd+M | F6 | Move to other panel |
| Cmd+N | F7 | Create directory |
| Cmd+D | F8 | Delete |

### Navigation

| Shortcut | Action |
|----------|--------|
| Tab | Switch active panel |
| Enter | Open directory / file |
| Backspace | Go to parent directory |
| Home / End | Jump to first / last entry |
| PageUp / PageDown | Scroll by page |
| Insert or Space | Toggle selection |

### Terminal & UI

| Shortcut | Action |
|----------|--------|
| Cmd+T | Toggle bottom terminal |
| Cmd+\` | Toggle Quake terminal |
| Cmd+Shift+T | Toggle in-pane terminal |
| Cmd+B | Toggle sidebar |
| Cmd+F | Search |
| Cmd+S | Connect to S3 |
| Cmd+Shift+L | Toggle dark/light theme |
| Cmd+Q | Quit |

## License

Furman is licensed under the [GNU General Public License v3.0](LICENSE).

Copyright (c) 2025 Bartosz Fenski
