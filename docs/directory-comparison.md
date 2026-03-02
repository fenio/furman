# Directory Comparison

Press **Cmd+Shift+D** (macOS) or **Ctrl+Shift+D** (Linux) to compare the left and right panels.

## How It Works

Comparison uses the existing `syncDiff` backend, which compares files between two directories using size and modification date.

### Status Colors

Files in each panel get a colored left border indicating their comparison status:

| Color  | Status   | Left Panel Meaning   | Right Panel Meaning    |
|--------|----------|----------------------|------------------------|
| Green  | New      | Only exists here     | Only exists here       |
| Yellow | Modified | Different from other | Different from other   |
| Red    | Deleted  | Missing (only other) | Missing (only other)   |
| None   | Same     | Identical            | Identical              |

Directories show aggregated status — if any child file differs, the directory is marked as modified.

### Filtering

The comparison bar shows count badges and filter buttons:
- **All** — Show all files
- **Only Here** — Show files that exist only in this panel (green)
- **Modified** — Show files that differ between panels (yellow)
- **Only There** — Show files missing from this panel (red)

## Supported Backends

| Left Panel | Right Panel | Supported |
|------------|-------------|-----------|
| Local      | Local       | Yes       |
| Local      | S3          | Yes       |
| S3         | Local       | Yes       |
| S3         | S3          | Yes       |
| SFTP       | Any         | No        |
| Archive    | Any         | No        |

## Controls

- **Cmd+Shift+D** — Toggle comparison on/off
- **Escape** — Stop comparison
- **×** button in comparison bar — Stop comparison
- Filter buttons in comparison bar — Filter visible entries
