# Error reporting

Errors should tell the user what operation failed and preserve available diagnostic
information, rather than showing only “error”, “service error”, or an object dump.

Use `ErrorNotice.svelte` for error views. Short messages stay inline. Multiline or
long messages have a concise summary and a **Details** disclosure, closed initially.
Details wrap, scroll, support keyboard navigation, and can be copied. Changing the
error closes the previous details. Render diagnostics as text, never HTML.

Use `errorText` when converting caught frontend errors to strings: it retains
messages, useful structured fields, and chained causes. It does not serialize
arbitrary object properties. If no details exist, say so rather than inventing a
cause or displaying `[object Object]`.

For S3 SDK failures, use `s3_sdk_error` with operation context. It distinguishes
service rejection, timeout, transport, request-construction, and invalid-response
failures, and includes available HTTP status, provider code/message, and request
IDs. Listing errors include the page; delete errors report completed work and
warn that the failed batch outcome is unknown. Do not change retry or timeout
behavior merely to improve diagnostics.

Never dump SDK Debug output, arbitrary headers, response bodies, credentials,
tokens, or encryption keys into user-visible diagnostics. Provider messages and
file paths can still identify private resources; review copied errors before
sharing them publicly. Possible causes must be described as possibilities, not
as a confirmed diagnosis.

New workflows should follow this pattern. Existing specialized error/status
views can migrate incrementally; this is not a global interception of every
exception, nor a guarantee that every provider returns useful diagnostics.
