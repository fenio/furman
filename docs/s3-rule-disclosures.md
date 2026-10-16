# S3 rule editor disclosure defaults

The inline **Lifecycle** and **CORS** rule lists use the same presentation policy:

- No rules: show the existing empty state and Add Rule action.
- One loaded rule: expand its settings by default.
- Multiple loaded rules: start with all rule bodies collapsed and show summaries.
- Expand/Collapse is always available, including for a sole rule. A manual choice
  survives field edits and tab changes; only one rule in each list is expanded at
  a time, preserving the existing lifecycle editor's behavior.
- Adding a rule explicitly opens the new form. Removing rules adjusts the selected
  index; if a single rule remains, it opens. Saving does not reset disclosure state.

These are view defaults, not configuration changes. Loading or toggling never
saves, removes or enables provider-side settings, and does not issue extra reads.
Existing capability gates, explicit Save/Delete actions, validation and service
payloads are unchanged. Keyboard activation of disclosure buttons does not bubble
into the properties dialog's Enter-to-close shortcut; Escape still closes normally.

Separate list/edit screens (inventory, replication, notifications, access points)
are not converted into auto-entered edit forms. This change applies to the existing
inline expandable CORS/lifecycle lists, not diagnostics or other disclosure defaults.

Synthetic controller/SSR tests cover zero/one/multiple loads, clean configuration
state, no writes on disclosure, explicit add/edit/removal behavior, index shifts,
keyboard handling and the actual rendered rule bodies and empty states.
