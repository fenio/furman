<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="src/lib/assets/furman-logotype-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="src/lib/assets/furman-logotype-light.svg">
    <img alt="Furman" src="src/lib/assets/furman-logotype-light.svg" width="450">
  </picture>
</p>

<p align="center">
  <b>F</b>ile & <b>U</b>RL <b>R</b>epository <b>MAN</b>ager<br>
  a dual-pane file manager for macOS and Linux inspired by times when Dos Navigator was the king.<br>
  <sub><i>furman</i> is also Polish for "carter" — one who hauls goods by horse-drawn cart, and in our case, hauls files.</sub>
</p>

<p align="center">
  <a href="https://github.com/fenio/furman/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/fenio/furman?style=flat-square"></a>
  <a href="LICENSE"><img alt="License: GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square"></a>
</p>

---

## Features

- **Dual-pane layout** with [per-panel tabs](docs/tabs.md) — work in multiple directories at once
- **[S3 cloud storage](docs/s3.md)** — connect to AWS, MinIO, Backblaze B2, Cloudflare R2, and 38+ providers
- **[SFTP remote access](docs/sftp.md)** — browse and manage files on any SSH server
- **Built-in terminal** — bottom panel, Quake-style drop-down, or inline per-pane
- **Git integration** — branch, status, and pull right in the panel header
- **[Batch rename](docs/batch-rename.md)** — find/replace, prefix/suffix, numbering, and case transforms
- **[Right-click context menu](docs/context-menu.md)** — all file operations one click away
- **Archive browsing** — navigate inside zip, rar, and 7z as if they were directories
- **View, edit, search** — built-in viewer, editor, and file/content search
- **Drag & drop, quick filter, auto-refresh** — everything you'd expect from a modern file manager
- **Dark & light themes** — follows your OS, or toggle with a shortcut

## Installation

### Homebrew

```sh
brew install fenio/tap/furman
```

### Download

Grab the latest `.dmg` (macOS) or `.AppImage` / `.deb` (Linux) from the [Releases](https://github.com/fenio/furman/releases/latest) page.

**macOS:** Both Apple Silicon (ARM) and Intel builds are available.

> **Note:** Furman is not signed with an Apple Developer certificate. On first launch macOS Gatekeeper will block it. To allow it, go to **System Settings > Privacy & Security** and click **Open Anyway**, or run:
> ```sh
> xattr -cr /Applications/Furman.app
> ```

**Linux:** Download the `.AppImage` and make it executable (`chmod +x Furman_*.AppImage`), or install the `.deb` package with `sudo dpkg -i Furman_*.deb`.

### Build from source

```sh
git clone https://github.com/fenio/furman.git
cd furman
npm install
npm run tauri build
```

The built package will be in `src-tauri/target/release/bundle/` (`.dmg` on macOS, `.AppImage`/`.deb` on Linux).

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
| Platform | macOS (Apple Silicon + Intel), Linux (x86_64) |

## Keyboard Shortcuts

A few highlights — see the [full reference](docs/keyboard-shortcuts.md) for every shortcut.

| Shortcut | Action |
|----------|--------|
| Tab | Switch panel |
| F2–F8 | Rename, View, Edit, Copy, Move, Mkdir, Delete |
| Cmd+S | Connect / disconnect S3 or SFTP |
| Cmd+T | Terminal |
| Cmd+Alt+T | New tab |
| Cmd+F | Search |
| Cmd+/ | Shortcut cheatsheet (in-app) |

## License

Furman is licensed under the [GNU General Public License v3.0](LICENSE).

Copyright (c) 2026 Bartosz Fenski
