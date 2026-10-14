# Inspecting comparison differences

Use **Compare Panes…** to scan local/local, local/S3, or S3/S3 panes. SHA-256 mode identifies payload differences; metadata mode compares size and timestamps.

Focus a modified file and click **Show diff** in the comparison bar, or right-click it and choose **Show diff with other pane**. Files are paired by the full path relative to the original comparison roots, so navigating into nested folders does not pair unrelated files with the same basename. The action is unavailable during scanning, for directories, and for files present on only one side.

The read-only viewer shows a unified line diff with left/right line numbers and missing-final-newline markers. It reads current file contents, not a snapshot retained from the scan. Identical current contents can therefore be reported even if the earlier comparison marked the file modified. S3 reads use each pane's own connection, including its SSE-C key, and may incur download charges. No temporary downloads are written to disk.

Limits: UTF-8 text, 512 KiB per side, 10,000 combined lines, and bounded diff-computation/read times. Binary or non-UTF-8 files are identified rather than decoded lossily. Larger or unsupported files should be downloaded and compared externally. Client-side password-encrypted ciphertext must be decrypted separately first.

Credential inputs also have a keyboard-accessible eye button. Fields start masked; revealing a field only changes its display, not the pasted value or how it is stored. Closing its dialog resets visibility. This applies to S3 secrets/SSE-C keys/proxy passwords, SFTP passwords/key passphrases, network-share passwords, and password prompts.
