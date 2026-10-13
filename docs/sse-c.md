# Customer-provided server-side encryption (SSE-C)

Edit an S3 connection and open its **Encryption** tab. Enable **Customer-provided server-side encryption (SSE-C)** and enter the original 32-byte AES-256 key used by the uploader. This is not the S3 access/secret credential pair. Terraform's `sse_customer_key` uses the base64 format; explicit hex and raw UTF-8 input are also supported.

**Save & Connect** stores the canonical base64 key in the OS keychain under a separate entry. Only an enabled flag is stored in the saved profile; the key is never added to the config JSON. **Connect Without Saving** keeps it only for the active connection. Stored Rust key buffers are zeroized on drop and their Debug output is redacted. HTTPS and normal certificate verification are required; key-bearing requests over HTTP are rejected.

The SDK signs the required customer algorithm/key/MD5 headers for object properties, downloads/previews, version reads, uploads, multipart uploads, and self-copy operations such as version restoration or metadata updates. Listing buckets, listing object versions and deleting objects do not receive encryption secrets.

A connection can also read unencrypted objects in a mixed bucket: if a read rejects the encryption headers with HTTP 400, it retries once without them under the SDK's standard retry policy. This fallback is **read-only**; uploads and copy destinations are never silently downgraded to unencrypted writes. Wrong or missing keys still cannot read encrypted data.

## Copying and verification

When either connection uses SSE-C, cross-connection copying uses download/upload relay. The source server decrypts with the source connection's key; the destination encrypts with its own connection's key if configured. This supports copying between different keys, or to/from an unencrypted destination/source. Configuring SSE-C therefore affects **new writes**, not just reads. Review the destination settings before copying.

SHA-256 comparison hashes retrieved plaintext bytes, so identical payloads can compare equal despite different encryption keys and ETags. SSE-C ETags are not assumed to be MD5 hashes of plaintext when downloading. Plain presigned links are disabled for SSE-C connections because recipients would also need encryption headers; keys are never put in generated links.

## Limits and error handling

- One SSE-C key is configured per connection. Use separate profiles for objects or historical versions encrypted with different keys. There is no key guessing or recovery; losing the original key makes the encrypted object unreadable.
- Object metadata cannot reliably reveal SSE-C without the right key. HTTP 400 alone is not proof of encryption. Failure messages suggest checking the original encryption key without asserting that every 400 is encryption-related.
- Ceph RGW uses the same “calculated MD5 hash of the key did not match” error for an inconsistent request key/MD5 pair **and** a supplied key whose MD5 differs from the object's stored key MD5. A locally verified key/MD5 pair therefore does not rule out a wrong object/version key. Do not change the MD5 encoding to work around this message.
- Server-side customer encryption is different from Furman's optional client-side/password encryption. If both were used, both layers' keys are required to obtain usable plaintext.
- This does not configure AWS KMS or migrate bucket encryption policy. Saved connections rely on the local OS keychain; exporting profiles does not export SSE-C secrets.

TLS MinIO regression tests cover correct/missing/wrong keys, application downloads, historical versions, restoration, copies between different keys, multipart relay and SHA-256 equality. SDK request-capture tests check signed headers, absent secrets on listing requests, and refusal to send keys over HTTP.

## Independent provider check (macOS)

`scripts/diagnose-sse-c.py` compares Boto3's automatic SSE-C encoding with explicit base64 key/MD5 fields using the same saved profile and keychain entries as Furman. It only issues HEAD and one-byte ranged GET requests, closes response bodies without reading them, and never prints keys or payloads. It does not modify buckets or objects. Python's memory is not guaranteed to be zeroized; use this short-lived process only as a local diagnostic.

Install Boto3 in a virtual environment, then run:

```sh
python scripts/diagnose-sse-c.py --profile 'YOUR SAVED PROFILE NAME' --key 'path/to/failing-object'
```

You may see macOS keychain prompts for both the login secret and the separate SSE-C key. Rebuilt dev executables may need renewed access permission too. Furman logs the purpose of each keychain read, but not its value or account ID.

If both Boto3 modes succeed while Furman fails on the same object/version, investigate Furman's request path. If both report the same MD5 error, the problem is not specific to Furman's interceptor: verify the original uploader's key and exact object version, while keeping provider-side behavior as a possibility. `--version-id` selects a specific version. Explicit proxies and assumed roles are deliberately rejected by this diagnostic rather than silently testing a different authentication path.
