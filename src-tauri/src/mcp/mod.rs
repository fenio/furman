pub mod state;
pub mod types;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use state::McpState;
use std::sync::atomic::AtomicBool;
use types::{
    S3ChangeStorageClassParams, S3ConnectParams, S3ConnectionIdParams, S3CopyObjectParams,
    S3DeleteObjectsParams, S3DownloadParams, S3ListObjectsParams, S3ObjectKeyParams,
    S3PresignUrlParams, S3PutObjectTagsParams, S3PutTextParams, S3UploadParams,
    SftpConnectParams, SftpConnectionIdParams, SftpCreateFolderParams, SftpDeleteParams,
    SftpListDirParams, SftpPutTextParams, SftpTransferParams, SftpUploadParams,
};

// ── FurmanMcp ───────────────────────────────────────────────────────────────

/// MCP server exposing Furman's S3 and SFTP capabilities as tools.
pub struct FurmanMcp {
    tool_router: ToolRouter<Self>,
    state: McpState,
}

impl Default for FurmanMcp {
    fn default() -> Self {
        Self {
            tool_router: Self::tool_router(),
            state: McpState::default(),
        }
    }
}

impl FurmanMcp {
    pub fn new() -> Self {
        Self::default()
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ── Tool Implementations ────────────────────────────────────────────────────

#[tool_router]
impl FurmanMcp {
    // ── S3 Operations ───────────────────────────────────────────────────

    /// Connect to an S3 bucket. Returns a connection_id for subsequent S3 operations
    #[tool(description = "Connect to an S3 bucket. Returns a connection_id for subsequent S3 operations")]
    async fn s3_connect(
        &self,
        Parameters(params): Parameters<S3ConnectParams>,
    ) -> Result<String, String> {
        let (client, sdk_config) = crate::s3::build_s3_client(
            &params.region,
            params.endpoint.as_deref(),
            params.profile.as_deref(),
            params.access_key.as_deref(),
            params.secret_key.as_deref(),
            None, None, None, None, None, None, None, None, None, None,
        )
        .await
        .map_err(err)?;

        let conn_id = uuid::Uuid::new_v4().to_string();
        let conn = crate::s3::S3Connection {
            client,
            bucket: params.bucket.clone(),
            region: params.region,
            sdk_config,
            account_id: None,
        };

        self.state
            .s3_connections
            .lock()
            .map_err(err)?
            .insert(conn_id.clone(), conn);

        Ok(format!(
            "Connected to bucket '{}'. connection_id: {conn_id}",
            params.bucket
        ))
    }

    /// Disconnect from an S3 bucket
    #[tool(description = "Disconnect from an S3 bucket")]
    async fn s3_disconnect(
        &self,
        Parameters(params): Parameters<S3ConnectionIdParams>,
    ) -> Result<String, String> {
        self.state
            .s3_connections
            .lock()
            .map_err(err)?
            .remove(&params.connection_id);
        Ok("Disconnected".into())
    }

    /// List all accessible S3 buckets for a connection
    #[tool(description = "List all accessible S3 buckets for a connection")]
    async fn s3_list_buckets(
        &self,
        Parameters(params): Parameters<S3ConnectionIdParams>,
    ) -> Result<String, String> {
        let client = {
            let map = self.state.s3_connections.lock().map_err(err)?;
            let conn = map
                .get(&params.connection_id)
                .ok_or("S3 connection not found")?;
            conn.client.clone()
        };
        let buckets = crate::s3::service::list_buckets(&client)
            .await
            .map_err(err)?;
        serde_json::to_string_pretty(&buckets).map_err(err)
    }

    /// List objects in an S3 bucket at the given prefix
    #[tool(description = "List objects in an S3 bucket at the given prefix")]
    async fn s3_list_objects(
        &self,
        Parameters(params): Parameters<S3ListObjectsParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let prefix = params.prefix.as_deref().unwrap_or("");
        let listing = svc.list_objects(prefix).await.map_err(err)?;
        serde_json::to_string_pretty(&listing).map_err(err)
    }

    /// Get metadata (head) of an S3 object
    #[tool(description = "Get metadata (head) of an S3 object")]
    async fn s3_head_object(
        &self,
        Parameters(params): Parameters<S3ObjectKeyParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let props = svc.head_object(&params.key).await.map_err(err)?;
        serde_json::to_string_pretty(&props).map_err(err)
    }

    /// Download an S3 object to a local file
    #[tool(description = "Download an S3 object to a local file")]
    async fn s3_download(
        &self,
        Parameters(params): Parameters<S3DownloadParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let keys = vec![params.key.clone()];
        svc.download(&keys, &params.local_path, "mcp", &cancel, &pause, &|_| {}, None)
            .await
            .map_err(err)?;
        Ok(format!("Downloaded '{}' to '{}'", params.key, params.local_path))
    }

    /// Upload a local file to S3
    #[tool(description = "Upload a local file to S3")]
    async fn s3_upload(
        &self,
        Parameters(params): Parameters<S3UploadParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let sources = vec![params.local_path.clone()];
        svc.upload(&sources, &params.key, "mcp", &cancel, &pause, &|_| {}, None)
            .await
            .map_err(err)?;
        Ok(format!("Uploaded '{}' to '{}'", params.local_path, params.key))
    }

    /// Delete one or more S3 objects
    #[tool(description = "Delete one or more S3 objects")]
    async fn s3_delete_objects(
        &self,
        Parameters(params): Parameters<S3DeleteObjectsParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        svc.delete_objects(&params.keys).await.map_err(err)?;
        Ok(format!("Deleted {} object(s)", params.keys.len()))
    }

    /// Copy an S3 object to a new key within the same bucket
    #[tool(description = "Copy an S3 object to a new key within the same bucket")]
    async fn s3_copy_object(
        &self,
        Parameters(params): Parameters<S3CopyObjectParams>,
    ) -> Result<String, String> {
        let (client, bucket) = {
            let map = self.state.s3_connections.lock().map_err(err)?;
            let conn = map
                .get(&params.connection_id)
                .ok_or("S3 connection not found")?;
            (conn.client.clone(), conn.bucket.clone())
        };
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let src_keys = vec![params.source.clone()];
        svc.copy_objects(
            &client, &bucket, &src_keys, &client, &bucket, &params.dest, "mcp", &cancel, &pause,
            &|_| {},
        )
        .await
        .map_err(err)?;
        Ok(format!("Copied '{}' to '{}'", params.source, params.dest))
    }

    /// Get tags on an S3 object
    #[tool(description = "Get tags on an S3 object")]
    async fn s3_get_object_tags(
        &self,
        Parameters(params): Parameters<S3ObjectKeyParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let tags = svc.get_object_tags(&params.key).await.map_err(err)?;
        serde_json::to_string_pretty(&tags).map_err(err)
    }

    /// Set tags on an S3 object
    #[tool(description = "Set tags on an S3 object")]
    async fn s3_put_object_tags(
        &self,
        Parameters(params): Parameters<S3PutObjectTagsParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        svc.put_object_tags(&params.key, &params.tags)
            .await
            .map_err(err)?;
        Ok(format!("Set {} tag(s) on '{}'", params.tags.len(), params.key))
    }

    /// Generate a presigned URL for an S3 object
    #[tool(description = "Generate a presigned URL for an S3 object")]
    async fn s3_presign_url(
        &self,
        Parameters(params): Parameters<S3PresignUrlParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        let expires = params.expires_secs.unwrap_or(3600);
        svc.presign_url(&params.key, expires)
            .await
            .map_err(err)
    }

    /// Change the storage class of an S3 object
    #[tool(description = "Change the storage class of an S3 object")]
    async fn s3_change_storage_class(
        &self,
        Parameters(params): Parameters<S3ChangeStorageClassParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        svc.change_storage_class(&params.key, &params.storage_class)
            .await
            .map_err(err)?;
        Ok(format!(
            "Changed '{}' storage class to '{}'",
            params.key, params.storage_class
        ))
    }

    /// Write text content directly to an S3 object
    #[tool(description = "Write text content directly to an S3 object")]
    async fn s3_put_text(
        &self,
        Parameters(params): Parameters<S3PutTextParams>,
    ) -> Result<String, String> {
        let svc = self.state.get_s3_service(&params.connection_id).map_err(err)?;
        svc.put_text(&params.key, &params.content)
            .await
            .map_err(err)?;
        Ok(format!("Wrote {} bytes to '{}'", params.content.len(), params.key))
    }

    // ── SFTP Operations ─────────────────────────────────────────────────

    /// Connect to an SFTP server. Returns a connection_id for subsequent SFTP operations
    #[tool(description = "Connect to an SFTP server. Returns a connection_id for subsequent SFTP operations")]
    async fn sftp_connect(
        &self,
        Parameters(params): Parameters<SftpConnectParams>,
    ) -> Result<String, String> {
        let port = params.port.unwrap_or(22);
        let conn = crate::sftp::client::build_sftp_client(
            &params.host,
            port,
            &params.username,
            &params.auth_method,
            params.password.as_deref(),
            params.key_path.as_deref(),
            None, None, None, None,
        )
        .await
        .map_err(err)?;

        let conn_id = uuid::Uuid::new_v4().to_string();
        self.state
            .sftp_connections
            .lock()
            .map_err(err)?
            .insert(conn_id.clone(), conn);

        Ok(format!(
            "Connected to {}:{port}. connection_id: {conn_id}",
            params.host
        ))
    }

    /// Disconnect from an SFTP server
    #[tool(description = "Disconnect from an SFTP server")]
    async fn sftp_disconnect(
        &self,
        Parameters(params): Parameters<SftpConnectionIdParams>,
    ) -> Result<String, String> {
        self.state
            .sftp_connections
            .lock()
            .map_err(err)?
            .remove(&params.connection_id);
        Ok("Disconnected".into())
    }

    /// List files and directories on an SFTP server
    #[tool(description = "List files and directories on an SFTP server")]
    async fn sftp_list_directory(
        &self,
        Parameters(params): Parameters<SftpListDirParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        let listing = svc.list_objects(&params.path).await.map_err(err)?;
        serde_json::to_string_pretty(&listing).map_err(err)
    }

    /// Download files from an SFTP server to a local directory
    #[tool(description = "Download files from an SFTP server to a local directory")]
    async fn sftp_download(
        &self,
        Parameters(params): Parameters<SftpTransferParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        let cancel = AtomicBool::new(false);
        svc.download(&params.remote_paths, &params.local_dest, "mcp", &cancel, &|_| {})
            .await
            .map_err(err)?;
        Ok(format!(
            "Downloaded {} file(s) to '{}'",
            params.remote_paths.len(),
            params.local_dest
        ))
    }

    /// Upload local files to an SFTP server
    #[tool(description = "Upload local files to an SFTP server")]
    async fn sftp_upload(
        &self,
        Parameters(params): Parameters<SftpUploadParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        let cancel = AtomicBool::new(false);
        svc.upload(&params.local_paths, &params.remote_dest, "mcp", &cancel, &|_| {})
            .await
            .map_err(err)?;
        Ok(format!(
            "Uploaded {} file(s) to '{}'",
            params.local_paths.len(),
            params.remote_dest
        ))
    }

    /// Delete files or directories on an SFTP server
    #[tool(description = "Delete files or directories on an SFTP server")]
    async fn sftp_delete(
        &self,
        Parameters(params): Parameters<SftpDeleteParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        svc.delete(&params.paths).await.map_err(err)?;
        Ok(format!("Deleted {} item(s)", params.paths.len()))
    }

    /// Create a directory on an SFTP server
    #[tool(description = "Create a directory on an SFTP server")]
    async fn sftp_create_folder(
        &self,
        Parameters(params): Parameters<SftpCreateFolderParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        svc.create_folder(&params.path).await.map_err(err)?;
        Ok(format!("Created folder: {}", params.path))
    }

    /// Write text content to a file on an SFTP server
    #[tool(description = "Write text content to a file on an SFTP server")]
    async fn sftp_put_text(
        &self,
        Parameters(params): Parameters<SftpPutTextParams>,
    ) -> Result<String, String> {
        let svc = self
            .state
            .get_sftp_service(&params.connection_id)
            .map_err(err)?;
        svc.put_text(&params.path, &params.content)
            .await
            .map_err(err)?;
        Ok(format!(
            "Wrote {} bytes to '{}'",
            params.content.len(),
            params.path
        ))
    }
}

// ── ServerHandler ───────────────────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for FurmanMcp {
    fn get_info(&self) -> ServerInfo {
        let caps = ServerCapabilities::builder().enable_tools().build();
        ServerInfo::new(caps)
            .with_instructions("Furman MCP server — S3 and SFTP tools for AI agents")
    }
}
