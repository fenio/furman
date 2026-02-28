<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="src/lib/assets/furman-logotype-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="src/lib/assets/furman-logotype-light.svg">
    <img alt="Furman" src="src/lib/assets/furman-logotype-light.svg" width="600">
  </picture>
</p>

<p align="center">
  <b>F</b>ile & <b>U</b>RL <b>R</b>epository <b>MAN</b>ager<br>
  a dual-pane file manager for macOS inspired by times when Dos Navigator was the king.<br>
  <sub><i>furman</i> is also Polish for "carter" — one who hauls goods by horse-drawn cart, and in our case, hauls files.</sub>
</p>

<p align="center">
  <a href="https://github.com/fenio/furman/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/fenio/furman?style=flat-square"></a>
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square"></a>
</p>

---

## Features

- **Dual-pane navigation** with Tab to switch between panes
- **[S3 support](S3.md)** — full-featured S3 client for 38+ S3-compatible providers (AWS, MinIO, Backblaze B2, Cloudflare R2, etc.) with multipart transfers, CRC32C checksum verification, versioning with MFA Delete, object lock, batch metadata/tag editing, lifecycle rules, CORS, bucket policies, client-side encryption (AES-256-GCM / ChaCha20), sync with exclude filters, bandwidth throttling, IAM role assumption, OIDC/Web Identity Federation, HTTP/HTTPS proxy support, CloudFront CDN management, inventory reports, replication configuration, event notifications, access points, anonymous access, and more
- **[SFTP support](SFTP.md)** — browse, transfer, view, and edit files on remote servers via SSH with password, SSH key, or SSH agent authentication. Cross-protocol transfers between local, S3, and SFTP
- **Integrated terminal** — bottom panel (Cmd+T), Quake-style drop-down (Cmd+\`), or in-pane mode (Cmd+Shift+T)
- **Git integration** — panel header shows repo indicator with branch name, ahead/behind status, dirty flag, pull button, and branch switcher
- **File viewer** (F3) — text with line numbers, image preview, hex dump
- **File editor** (F4) — built-in text editor with dirty-state tracking
- **Search** (Cmd+F) — search by file name or file content with streaming results
- **Archive browsing** — navigate inside zip, rar, and 7z archives as if they were directories (requires `brew install 7zip`)
- **File watcher** — panels auto-refresh when files change on disk
- **Quick filter** — type to filter the file list in real time
- **Directory sizes** — selecting a directory calculates its recursive size (configurable in Preferences)
- **Selection** — click, Cmd+click to toggle, Shift+click for range, or rubber-band (marquee) drag in empty space
- **Drag and drop** — drag files between panels to copy (or Shift+drag to move)
- **Sidebar** — favorites, workspaces, S3 bookmarks, SFTP bookmarks, mounted devices, active S3/SFTP connections, theme toggle (Cmd+B to open, press again to focus for keyboard navigation)
- **Preferences** — icon size, hidden files, external editor, startup sound (accessible via F9 menu)
- **Dark / Light theme** — auto-detects OS preference, toggle with Cmd+Shift+L
- **List, icon, and column views** — switch between list, grid, and column layouts with configurable icon sizes

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

## Screenshots

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/user-attachments/assets/0fc576cf-b13d-4db2-955d-0e42f2b52864">
  <source media="(prefers-color-scheme: light)" srcset="https://github.com/user-attachments/assets/de277ee1-edb9-41b1-8fd8-bf2b3c3c0dc5">
  <img alt="Furman dual-pane file manager" src="https://github.com/user-attachments/assets/0fc576cf-b13d-4db2-955d-0e42f2b52864">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/user-attachments/assets/45465cb2-04c8-4dcd-9ab5-d529e0ef323d">
  <source media="(prefers-color-scheme: light)" srcset="https://github.com/user-attachments/assets/40f77b47-833a-4580-be66-160eb60c91bd">
  <img alt="Furman with terminal and S3" src="https://github.com/user-attachments/assets/45465cb2-04c8-4dcd-9ab5-d529e0ef323d">
</picture>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://github.com/user-attachments/assets/ad218d5c-3285-4a8d-95e3-ca534d8ba5ad">
  <source media="(prefers-color-scheme: light)" srcset="https://github.com/user-attachments/assets/b395210c-cd5a-4726-9ade-6079e61b7916">
  <img alt="Furman icon view" src="https://github.com/user-attachments/assets/ad218d5c-3285-4a8d-95e3-ca534d8ba5ad">
</picture>



## Tech Stack

| Layer    | Technology |
|----------|------------|
| Frontend | SvelteKit 5, TypeScript, Vite |
| Backend  | Rust, Tauri 2 |
| Terminal | xterm.js |
| S3       | aws-sdk-s3 (Rust) |
| SFTP     | russh, russh-sftp (Rust) |
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
| Cmd+Backspace | F8 | Delete |

### Navigation

| Shortcut | Action |
|----------|--------|
| Tab | Switch active panel |
| Enter | Open directory / file |
| Backspace | Go to parent directory |
| Home / End | Jump to first / last entry |
| PageUp / PageDown | Scroll by page |
| Insert or Space | Toggle selection |

### Connections

| Shortcut | Action |
|----------|--------|
| Cmd+S | Connection Manager / disconnect S3 or SFTP |
| Cmd+D | Bookmark S3 or SFTP path / Save workspace (local) |
| Cmd+I | Properties / Connection info |
| Cmd+Y | Sync between panels |

### S3

| Shortcut | Action |
|----------|--------|
| Cmd+U | Presigned URL to clipboard |
| Cmd+K | Copy S3 URI to clipboard |
| Cmd+L | Bulk storage class change |
| Cmd+Shift+I | Bucket properties |

### Terminal & UI

| Shortcut | Action |
|----------|--------|
| Cmd+T | Toggle bottom terminal |
| Cmd+\` | Toggle Quake terminal |
| Cmd+Shift+T | Toggle in-pane terminal |
| Cmd+B | Toggle sidebar (press twice to focus for keyboard navigation) |
| Cmd+J | Toggle transfer panel |
| F9 | Toggle menu |
| Cmd+F | Search |
| Cmd+Shift+L | Toggle dark/light theme |
| Cmd+Q | Quit |

## License

Furman is licensed under the [GNU General Public License v3.0](LICENSE).

Copyright (c) 2026 Bartosz Fenski
