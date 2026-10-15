# AWS shared profiles and SSO

Furman uses the existing AWS SDK credential chain. This change adds local
shared-profile discovery and an explicitly approved AWS CLI v2 sign-in action;
it does not replace authentication or add a token store.

## Discovery and profile selection

- Only AWS connections without a custom endpoint, anonymous auth or OIDC show
  the tools. Linode/Akamai and other S3-compatible connection paths are unchanged.
- Discovery reads config/credentials using SDK profile parsing and merge rules,
  honoring `AWS_CONFIG_FILE`, `AWS_SHARED_CREDENTIALS_FILE`, and home expansion.
  It never resolves credentials, executes `credential_process`, contacts AWS,
  or inspects the SSO cache. Discovery is not a session-validity check.
- Files must be regular UTF-8 files, at most 2 MiB each; discovery has a
  five-second deadline and a 2,000-profile limit. Failures show safe diagnostics,
  not file contents. Missing files yield no profiles.
- The report contains only profile names, S3 region, SSO eligibility and safe
  static explanations. It does not export account IDs, role ARNs, start URLs,
  credential values, or credential-process commands.
- Selecting a discovered profile explicitly clears manual keys in the form.
  Other form fields remain unchanged. Region adoption is separate and uses
  `region`, not `sso_region`. Manual keys otherwise retain their existing
  precedence; signing in is disabled while manual keys are present.

## Explicit sign-in

- Modern SSO sessions, legacy SSO profiles, and bounded source-profile role
  chains are discovered. The UI displays both the selected profile and actual
  SSO source profile before approval. Missing/cyclic/incomplete configuration,
  mixed credential sources and custom endpoint/services configuration fail
  closed for the sign-in action; ordinary SDK connections remain available.
- Approval launches a local `aws` executable, checks for CLI v2, and runs
  `aws sso login --profile=<source>` with literal arguments, never a shell.
  Absolute PATH directories and standard macOS installation locations are
  searched. Installation/version/launch errors are visible.
- Configuration is re-read and source-profile identity rechecked before launch.
  AWS/SSO environment endpoint overrides block this convenience action rather
  than silently changing the CLI's endpoint policy.
- Only one sign-in runs at a time. Cancel and the five-minute deadline stop
  the CLI child. Closing the UI requests cancellation. Browser tabs or
  AWS-managed cache updates already made may remain.
- AWS CLI may open a browser and manages its own tokens. Furman does not
  capture login output, authorization URLs/codes, or token files. Failure
  diagnostics include the exit status and safe troubleshooting guidance;
  run the CLI externally if raw CLI diagnostics are needed.
- CLI environment/proxy settings are distinct from Furman's connection proxy.
  Sign-in clears display caches, but never reconnects, retries transfers,
  changes bucket permissions or modifies existing SDK clients automatically.
  Explicitly reconnect to reload credentials.

## Error boundaries and exclusions

Typed credential-provider failures with SSO-specific evidence distinguish
expired, unavailable/invalid cached sessions and configuration problems.
Provider HTTP/service errors retain their codes, status and permission meaning:
Linode/Akamai `AccessDenied`, `ExpiredToken`, `InvalidAccessKeyId` and
`SignatureDoesNotMatch` are not labeled AWS SSO expiry. Saved eligible AWS
connections offer sign-in recovery for recognized SSO failures.

No automatic sign-in, profile creation, token browsing/storage, default-chain
identity guessing, permission repair, or scheduling is included. Shared files
can change between inspection and CLI/SDK reads; this is not a configuration
snapshot or a sandbox for a malicious local CLI installation.
