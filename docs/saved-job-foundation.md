# Saved job definitions — configured-identity foundation

This is the first implementation step of the
[pinned manual-job design](pinned-sync-jobs-design.md). **Jobs cannot run yet.**
There are no job connect, scan, preview, execute, retry, history or scheduling
commands. `execution_available` is always false, and the UI's Run control is
permanently disabled, not merely hidden behind a frontend condition.

## Using definitions

In Sync, expand **Saved job definitions — no execution**. These fields are
separate from the current Sync operation: selecting, saving or rebinding does
not apply a preset, change the current roots/options, clear or approve its scan,
connect, resolve credentials, read the keychain, or transfer data.

Choose a local source directory or saved S3 source profile, a saved S3
destination profile, and explicit exclusions. Review the displayed source →
destination and acknowledge saving configuration pins. Save a new definition,
or select one and explicitly **Save edits / rebind**. Deletion requires its own
confirmation and deletes only the definition, not data or connection profiles.
Refresh clears selection and acknowledgement; errors retain the last confirmed
list and do not imply a save succeeded.

Native commands resolve profile references from the persisted
`~/.config/furman/config.json`; caller-supplied endpoint/configuration pins are
not accepted. Profiles only present in an unsaved frontend form cannot be used.
Save requests carry both a store revision and the reviewed non-secret
configuration revision; stale managed generations or externally edited endpoint
configuration require refresh and another explicit review.

## What is pinned

- Directional definition UUID, monotonically increasing definition revision
  (serialized as a decimal string), name, source, S3 destination, explicit
  exclusions, fixed SHA-256 mode and fixed `copy_update_only` intent.
- For S3: saved profile ID, random managed generation, declared bucket/region,
  canonical root endpoint URL, provider, explicit proxy endpoint/auth-presence
  flag, SSE-C enablement and acceleration intent. Proxy usernames, access-key
  IDs and all secret values are absent from this projection.
- For local sources: absolute no-follow directory path and device/inode drift
  indicators, serialized as strings. Each component is opened relative to the
  previous directory without following symlinks; files and special nodes are
  rejected. No contents are read. Listing checks whether that directory still
  matches; a replaced/unavailable root needs rebind.

New S3 connections, including MCP-created connections, also receive a private,
immutable native client-instance UUID. It is never persisted as a job pin and
is **not evidence of a profile or principal binding**. Existing native clients,
SSE-C interceptors, provider gates and SDK request configuration are preserved.

`definition_valid` means only that the saved configuration/generations and
local metadata matched at inspection. It does not mean runnable, authenticated,
connected or verified. A missing endpoint is **declared AWS region intent**, not
proof of effective routing. This step never loads SDK profiles, consults endpoint
overrides through credential resolution, or makes discovery/probe requests.

The next pipeline must still implement verified live saved-profile/keychain
binding, effective endpoint/transport checks, native run ownership, shared
retained local-root descriptors, complete manifests and guarded approvals.
Reopening a path separately for comparison/upload or matching a display label
must not be treated as having completed those gates. Local descriptors in this
foundation live only for the metadata inspection; file IDs can be reused.

## Eligibility and namespace checks

Definitions initially accept saved manual-key (`keychain`) S3 profiles with
simple stable IDs. Default/shared-profile/SSO/OIDC/anonymous auth, mixed or
assumed-role settings, system-proxy routing and client-side password encryption
are unavailable in this editor. This does not remove their ordinary connection,
comparison or transfer support. No SSO tokens are inspected or stored.

Custom endpoints/proxies must be root HTTP(S) URLs without userinfo, backslashes,
non-root paths, query strings or fragments. Unsafe URLs are rejected, not
redacted into a different namespace. SSE-C intent with an explicit HTTP endpoint
is rejected; keys remain connection-local and keychain-backed.

Prefixes are literal, including Unicode, leading/repeated slashes and `.`/`..`.
Empty means bucket root. As a foundation refinement, nonempty **folder** prefixes
must explicitly end in `/`; no slash is silently appended or removed. Equal or
ancestor/descendant prefixes in the same configured endpoint/bucket are refused
even across different profile IDs. Adjacent names such as `data/` and `database/`
are distinct. This preliminary configuration check cannot discover arbitrary
DNS/proxy aliases and is not the future live namespace authorization gate.

Garage definitions can be recorded, but cannot execute here; future execution
must retain native conditional-write gates and the signing backstop. No job
definition can store an unguarded overwrite policy, deletion, approval, schedule,
results, selected files or runtime connection IDs.

## Managed edits and persistence

`~/.config/furman/saved-jobs.json` stores strict version-1 definitions and random
per-profile generation markers. It is separate from general preferences and
options presets. Bounds: 100 definitions, 4,096 tracked generation IDs and 1 MiB
total; configuration inspection is limited to 4 MiB / 2,000 profiles. Names are
unique ignoring case, 1–80 characters; exclusions are explicit, at most 128
patterns of 512 bytes, following the existing comma-separated UI syntax.

Supported native `keychain_set`/`keychain_delete` operations track the base
profile, proxy, SSE-C and OIDC-refresh entry suffixes. A generation is random,
**never a hash of credentials or encryption keys**. Managed configuration saves
compare profile values in memory to detect access-key-ID/auth/transport changes;
these values are not copied into generation storage or a credential-derived
fingerprint. Display-name-only and unrelated preference edits do not invalidate
pins. Configuration-review hashes include only the allowlisted public projection.
Legacy credential IDs outside the job schema remain usable ordinarily.

Mutation protocol:

1. Acquire a nonblocking, cooperating-process storage lock.
2. Persist a fresh random generation marked pending, flushing the file and its
   directory, **before** the keychain/configuration mutation.
3. Perform the requested ordinary edit. Mark the new generation ready only after
   success. A failed or ambiguous edit leaves a pending marker and old job pins
   invalid; successfully resave the connection/secret, then explicitly rebind.

There is no transaction spanning the OS keychain and JSON files. If storage
cannot safely record invalidation, the managed edit is refused rather than
silently leaving old pins valid. Corrupt/future/unreadable job storage can
therefore block managed credential edits and preference saves. Existing
credential reads, connections and transfers are not changed or automatically
revoked. Repair/back up the indicated storage externally; do not delete tracking
metadata merely to make an old job look valid. No automatic reset is provided.

General configuration saves through the normal native configuration path now
use the same lock and private atomic replacement, not truncate-in-place writes.
Store/configuration temporary files use mode 0600, content flush, same-directory
rename and directory flush. Unsupported locking/flush semantics fail visibly.
This is identity invalidation infrastructure, **not** a shipped run/outcome
journal or a universal power-loss/hostile-filesystem guarantee. External editors
that ignore the lock, direct external keychain edits and alternative file APIs
are outside managed generation tracking. Pins are configuration drift checks,
not proof of a principal, bucket ownership or physical server identity.

Errors never include parsed file contents or rejected credential-bearing URLs.
Job errors use copyable, initially collapsed detailed diagnostics; collapsed
sections signal failure in their summary. A global nonmodal notice makes failed
configuration saves visible even for unawaited preference callers, without
swallowing the promise rejection required by Save & Connect. A later successful
save clears it, and stale completion errors cannot replace a newer result.
Connection deletion also stops and shows copyable diagnostics if keychain
cleanup fails, rather than logging and silently removing the profile. Earlier
credential deletions in that attempt may already have succeeded; the message
explicitly warns about partial cleanup.

## Explicitly deferred

Native credential/effective-namespace binding for runs, retained manifests,
source descriptors shared across scanning and snapshotting, execution policies,
post-write verification, durable attempt history, review pauses, restart
reconciliation and scheduling remain follow-up work. No definition created by
this step authorizes a later writer; the remaining design acceptance tests are
mandatory before execution controls can be enabled.
