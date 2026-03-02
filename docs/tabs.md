# Tabs

Furman supports multiple tabs per panel, letting you keep several directories open side-by-side without losing your place.

## Opening & Closing Tabs

| Action | How |
|--------|-----|
| New tab | **Cmd+Alt+T** — opens a new tab in the active panel, starting at the same directory |
| Close tab | **Cmd+Alt+W** or click the **x** button on the tab |
| Close tab (mouse) | Middle-click on a tab |

The tab bar appears automatically when a panel has more than one tab. With a single tab the bar is hidden to save space.

## Switching Tabs

Click any tab to switch to it. The active tab is highlighted with an accent underline. Each tab maintains its own directory, cursor position, selection, and scroll state independently.

## Tab Labels

Tab labels show the current directory name:
- **Local** — last path segment (e.g. `Documents`)
- **S3** — last prefix segment, or the bucket name at root
- **SFTP** — last path segment, or the hostname at root

## Workspaces

Tabs are saved and restored with workspaces. When you save a workspace (**Cmd+D**), all open tabs and active tab indices are persisted. Restoring a workspace reopens the same tabs in the same layout.

Workspaces saved before tabs were introduced still work — they restore as a single tab per panel.
