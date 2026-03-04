use crate::models::FmError;
use crate::s3::{S3Connection, S3Service};
use crate::sftp::{SftpConnection, SftpService};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared state for MCP server connections.
pub struct McpState {
    pub s3_connections: Arc<Mutex<HashMap<String, S3Connection>>>,
    pub sftp_connections: Arc<Mutex<HashMap<String, SftpConnection>>>,
}

impl Default for McpState {
    fn default() -> Self {
        Self {
            s3_connections: Arc::new(Mutex::new(HashMap::new())),
            sftp_connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl McpState {

    /// Get an S3Service for the given connection ID.
    pub fn get_s3_service(&self, id: &str) -> Result<S3Service, FmError> {
        let map = self
            .s3_connections
            .lock()
            .map_err(|e| FmError::Other(e.to_string()))?;
        let conn = map
            .get(id)
            .ok_or_else(|| FmError::NotFound(format!("S3 connection '{id}' not found")))?;
        Ok(S3Service {
            client: conn.client.clone(),
            bucket: conn.bucket.clone(),
        })
    }

    /// Get an SftpService for the given connection ID.
    pub fn get_sftp_service(&self, id: &str) -> Result<SftpService, FmError> {
        let map = self
            .sftp_connections
            .lock()
            .map_err(|e| FmError::Other(e.to_string()))?;
        let conn = map
            .get(id)
            .ok_or_else(|| FmError::NotFound(format!("SFTP connection '{id}' not found")))?;
        Ok(SftpService {
            session: conn.session.clone(),
            host: conn.host.clone(),
            port: conn.port,
            operation_timeout_secs: conn.operation_timeout_secs,
        })
    }
}
