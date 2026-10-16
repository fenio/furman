# Read-only viewer highlighting

The viewer and quick preview use the existing bundled highlight.js grammars;
no new dependency, network service or file parser/formatter is introduced.

- Terraform `.tfstate`, `.tfstate.backup` and `.tfstate.lock.info` files are
  explicitly JSON, avoiding unreliable language guesses based on their values.
  Detection uses the basename case-insensitively, including temporary-file paths.
- JSON property names are blue, strings green, numbers amber, boolean/null
  literals violet, and punctuation neutral. Both dark/light palettes have distinct
  roles and at least 4.5:1 contrast against the viewer background. The same token
  colours apply in quick preview; line numbers in the viewer are less faint.
- Text-mode Viewer has a **Syntax** picker: automatic filename/content detection,
  Plain text, or any installed highlight.js grammar. Selection is per viewer open,
  not persisted. It only recolours already-loaded content; it does not reread,
  download, write, reformat or parse/reorder JSON. Known `.txt`, `.log` and `.csv`
  files default to plain text; users can override that explicitly.
- Numbered lines close/reopen multiline token spans at their boundaries, keeping
  comments and strings coloured across lines without broken HTML. Quick preview
  already renders a continuous fragment and needs no such line wrapping.
- Source markup stays escaped before HTML insertion. Search, line numbers,
  whitespace and original text are preserved; native select keyboard navigation
  does not trigger viewer close/scroll shortcuts. Hex/image modes have no syntax
  control and retain their existing behavior.

This is highlighting, not syntax validation or a JSON tree editor. Malformed files
are still displayed, not repaired. Automatic content detection remains heuristic
for unrecognized filenames. Only installed grammars are offered; an old filename
mapping to an unavailable grammar (such as Terraform/HCL) is not falsely displayed
as a supported syntax mode. Existing file-loading limits/behavior are unchanged.

Tests cover state/backup detection, semantic JSON tokens, both palette contrasts,
multiline span balance, escaped markup, exact text preservation, real Viewer
controller recolouring without extra I/O, keyboard isolation, search coexistence
and text-only picker rendering. Fixtures use synthetic contents, not user state
files or production credentials.
