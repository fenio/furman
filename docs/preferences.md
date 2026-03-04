# Preferences

Open Preferences with **Cmd+,** (or via the Command Palette). Settings are organized into four tabs and persist across sessions.

## General

| Setting | Options | Default |
|---------|---------|---------|
| **Show Hidden Files** | on / off | off |
| **Calculate Directory Sizes** | on / off | off |
| **Play Startup Sound** | on / off | on |
| **Quick Filter on Keystroke** | on / off | on |
| **Image Tooltips on Hover** | on / off | on |
| **Confirm Before Overwrite** | Ask / Always overwrite / Never overwrite | Ask |
| **External Editor** | path to editor binary | (empty — uses built-in) |
| **Diagnostic Log Files** | open log directory | — |

**Quick Filter** — when enabled, typing any letter in a file panel immediately filters the listing. Disable to use keyboard shortcuts that start with letter keys.

**Confirm Before Overwrite** — controls what happens when a copy or move would overwrite existing files:
- *Ask* — show a conflict dialog (default)
- *Always overwrite* — overwrite without asking
- *Never overwrite* — skip conflicting files silently

## Appearance

| Setting | Options | Default |
|---------|---------|---------|
| **Theme** | Dark / Light | follows OS |
| **Default View Mode** | List / Icon / Column | List |
| **Date Format** | ISO / EU / US / Relative | ISO |
| **Row Height** | Compact / Normal / Comfortable | Normal |
| **Icon Size** | Small / Medium / Large | Medium |

**Date Format** examples:
- *ISO* — `2026-03-04 14:30`
- *EU* — `04.03.2026 14:30`
- *US* — `03/04/2026 2:30 PM`
- *Relative* — `5m ago`, `3h ago`, `2d ago`

**Row Height** affects both list and column views. Compact is useful for seeing more files at once; Comfortable gives more breathing room.

## Network

### S3

| Setting | Options | Default |
|---------|---------|---------|
| **Concurrent Transfers** | 1 / 2 / 3 / 4 / 5 | 3 |
| **Bandwidth Limit** | Unlimited / 1–100 MB/s | Unlimited |
| **Multipart Upload Threshold** | 8 / 16 / 32 / 64 MB | 8 MB |
| **Multipart Part Size** | 8 / 16 / 32 / 64 MB | 8 MB |
| **Concurrent Parts** | 2 / 4 / 8 / 12 | 4 |
| **Secure Temp File Cleanup** | on / off | off |

**Multipart settings** control how large files are uploaded to S3. Files larger than the threshold are split into parts of the configured size and uploaded with the specified concurrency. Higher concurrency can improve throughput on fast connections.

### SFTP

| Setting | Options | Default |
|---------|---------|---------|
| **Inactivity Timeout** | 5 / 10 / 30 / 60 min | 5 min |
| **Keepalive Interval** | 15 / 30 / 60 / 120 s | 30 s |
| **Operation Timeout** | 30 / 60 / 120 / 300 s | 60 s |

SFTP timeout settings apply to new connections only — changing them does not affect already-open sessions.

- **Inactivity Timeout** — disconnect after this period of no activity
- **Keepalive Interval** — send keepalive packets at this interval to prevent the server from dropping idle connections
- **Operation Timeout** — maximum time to wait for a single SFTP operation (download, upload, listing, etc.)

## Terminal

| Setting | Options | Default |
|---------|---------|---------|
| **Font Size** | 10–18 px | 13 px |
| **Scrollback Lines** | 1,000 / 5,000 / 10,000 / 50,000 | 5,000 |
| **Custom Shell Path** | path to shell binary | (empty — auto-detect) |

**Font Size** changes take effect immediately in all open terminals.

**Scrollback Lines** and **Custom Shell Path** apply to new terminal tabs only.

When the shell path is empty, Furman uses `$SHELL` on macOS/Linux, or the platform default if unset.
