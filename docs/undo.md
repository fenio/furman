# Undo / Operation History

Press **Cmd+Z** (macOS) or **Ctrl+Z** (Linux) to undo the last operation.

## Supported Operations

| Operation | Undo Action                    | Backend Support |
|-----------|--------------------------------|-----------------|
| Delete    | Restore from trash             | Local only      |
| Rename    | Rename back to original name   | Local only      |

## How It Works

### Delete Undo (macOS)
- Uses `NSFileManager.trashItemAtURL` to move files to trash
- Records the resulting trash URL for each file
- Undo moves files from trash back to their original locations

### Delete Undo (Linux)
- Uses the `trash` crate to move files to the freedesktop trash
- Undo support is limited on Linux

### Rename Undo
- Records original path and new name
- Undo calls `renameFile(newPath, originalName)` to reverse the operation

## Toast Notification
- After each operation, a toast appears above the function bar
- Shows operation summary (e.g., "Deleted 3 file(s)")
- "Undo" button appears only for local backend operations
- Auto-dismisses after 8 seconds
- Click "Undo" or press Cmd+Z to reverse

## Limitations
- Maximum 20 operations in history (in-memory only, not persisted)
- S3 and SFTP deletions cannot be undone (no trash concept)
- Move operations are not currently undoable
- History is cleared when the app is closed
