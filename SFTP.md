# SFTP Support

Furman includes a built-in SFTP client for browsing, transferring, viewing, and editing files on remote servers over SSH.

## Connecting

Press **Cmd+S** to open the Connection Manager, then click **SFTP** in the sidebar. You can authenticate using:

- **Password** — stored securely in macOS Keychain
- **SSH Key** — specify a path to a private key file (e.g. `~/.ssh/id_rsa`) with optional passphrase
- **SSH Agent** — use keys loaded in your running SSH agent

Enter the host, port (default 22), and username, then click **Connect** or **Save & Connect** to store the profile for later use.

Saved SFTP connections appear in the Connection Manager sidebar. Press **Cmd+S** again while connected to disconnect.

## Browsing & Navigation

- Browse remote directories with standard dual-pane navigation
- Breadcrumb bar shows `user@host` with clickable path segments
- SFTP icon badge in the panel header — click it for connection info
- Create, rename, and delete files and directories
- Quick filter works the same as local panels

## Transfers

- **Download** (SFTP → local) and **upload** (local → SFTP) with Copy/Move commands or OS drag-and-drop
- **SFTP-to-SFTP copy** between two SFTP connections (via local temp directory)
- **Cross-protocol transfers** — copy between S3 and SFTP in either direction
- **Transfer queue** with the same progress tracking, pause/resume, and bandwidth controls as S3

## Viewing & Editing

- **View** (F3 / Cmd+3) — downloads the remote file to a temp directory and opens the viewer (text, image, or hex)
- **Edit** (F4 / Cmd+E) — downloads the remote file, opens the built-in editor, and saves changes back to the server on Cmd+S

## Sync

Press **Cmd+Y** to sync between an SFTP panel and a local or SFTP panel. Supports the same diff view, exclude patterns, and selective transfer as S3 sync.

## Bookmarks

Press **Cmd+D** while browsing an SFTP connection to bookmark the current path. Bookmarks appear in the sidebar under **SFTP BOOKMARKS** and reconnect automatically when clicked.

## Properties

Press **Cmd+I** on a remote file to view its properties (size, modified date, permissions, owner, group). Click the SFTP icon in the panel header to view connection info (host, port, username, protocol).

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Cmd+S | Connection Manager / disconnect SFTP |
| Cmd+D | Bookmark current SFTP path |
| Cmd+I | File properties / connection info |
| Cmd+Y | Sync between panels |
| F3 / Cmd+3 | View remote file |
| F4 / Cmd+E | Edit remote file |
| F5 / Cmd+C | Copy to other panel |
| F6 / Cmd+M | Move to other panel |
| F7 / Cmd+N | Create directory |
| F8 / Cmd+Backspace | Delete |
