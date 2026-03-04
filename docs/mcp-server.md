# Furman MCP Server

Furman ships a standalone MCP (Model Context Protocol) server that exposes S3 and SFTP operations as tools for AI agents. It runs as a separate binary — no GUI or Tauri runtime needed.

## Building

```sh
cd src-tauri
cargo build --bin furman-mcp --features mcp --release
```

The binary will be at `src-tauri/target/release/furman-mcp`.

## Setup

### Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "furman": {
      "command": "/path/to/furman-mcp"
    }
  }
}
```

### Claude Code

Add to `.claude/settings.json` or run:

```sh
claude mcp add furman /path/to/furman-mcp
```

### Other MCP clients

Any MCP client that supports stdio transport can use furman-mcp. Point it at the binary with no arguments — it communicates over stdin/stdout using JSON-RPC.

## Tools

### S3 (14 tools)

All S3 tools use a connection-based workflow: connect first, then use the returned `connection_id` for all operations.

| Tool | Description |
|------|-------------|
| `s3_connect` | Connect to an S3 bucket (AWS, MinIO, R2, B2, etc.). Returns a `connection_id` |
| `s3_disconnect` | Close an S3 connection |
| `s3_list_buckets` | List all accessible buckets |
| `s3_list_objects` | List objects at a prefix |
| `s3_head_object` | Get object metadata (size, content type, storage class, etc.) |
| `s3_download` | Download an object to a local file |
| `s3_upload` | Upload a local file to S3 |
| `s3_delete_objects` | Delete one or more objects |
| `s3_copy_object` | Copy an object to a new key within the same bucket |
| `s3_get_object_tags` | Get tags on an object |
| `s3_put_object_tags` | Set tags on an object |
| `s3_presign_url` | Generate a presigned download URL |
| `s3_change_storage_class` | Change storage class (Standard, IA, Glacier, etc.) |
| `s3_put_text` | Write text content directly to an object |

#### Example: S3 workflow

```
1. s3_connect(bucket: "my-bucket", region: "us-east-1")
   → "Connected to bucket 'my-bucket'. connection_id: abc-123"

2. s3_list_objects(connection_id: "abc-123", prefix: "data/")
   → JSON listing of objects

3. s3_download(connection_id: "abc-123", key: "data/report.csv", local_path: "/tmp/report.csv")
   → "Downloaded 'data/report.csv' to '/tmp/report.csv'"

4. s3_disconnect(connection_id: "abc-123")
   → "Disconnected"
```

### SFTP (8 tools)

Same connection-based pattern as S3.

| Tool | Description |
|------|-------------|
| `sftp_connect` | Connect to an SSH/SFTP server (password or key auth). Returns a `connection_id` |
| `sftp_disconnect` | Close an SFTP connection |
| `sftp_list_directory` | List files and directories at a remote path |
| `sftp_download` | Download remote files to a local directory |
| `sftp_upload` | Upload local files to a remote directory |
| `sftp_delete` | Delete remote files or directories |
| `sftp_create_folder` | Create a remote directory |
| `sftp_put_text` | Write text content to a remote file |

#### Example: SFTP workflow

```
1. sftp_connect(host: "server.example.com", username: "deploy", auth_method: "key", key_path: "~/.ssh/id_ed25519")
   → "Connected to server.example.com:22. connection_id: def-456"

2. sftp_list_directory(connection_id: "def-456", path: "/var/www")
   → JSON listing of files

3. sftp_upload(connection_id: "def-456", local_paths: ["/tmp/index.html"], remote_dest: "/var/www/")
   → "Uploaded 1 file(s) to '/var/www/'"

4. sftp_disconnect(connection_id: "def-456")
   → "Disconnected"
```

## Authentication

### S3

`s3_connect` supports multiple auth methods:

- **Default credential chain** — just provide `bucket` and `region`; uses `~/.aws/credentials`, environment variables, or instance roles
- **Named profile** — set `profile: "my-profile"` to use a specific AWS profile
- **Access keys** — provide `access_key` and `secret_key` directly
- **Custom endpoint** — set `endpoint` for MinIO, Cloudflare R2, Backblaze B2, etc.

### SFTP

`sftp_connect` supports:

- **Password** — set `auth_method: "password"` and provide `password`
- **Key-based** — set `auth_method: "key"` and provide `key_path`

## Debugging

Enable trace logging by setting the `RUST_LOG` environment variable:

```sh
RUST_LOG=debug furman-mcp
```

Logs go to stderr (stdout is reserved for MCP protocol messages).
