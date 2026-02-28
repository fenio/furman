use crate::models::FmError;

/// Convenience constructor for SFTP errors.
pub fn sftperr(msg: impl Into<String>) -> FmError {
    FmError::Sftp(msg.into())
}

/// Build an `sftp://host:port/path` URI.
pub fn sftp_path(host: &str, port: u16, path: &str) -> String {
    format!("sftp://{}:{}{}", host, port, path)
}

/// Parse an `sftp://host:port/path` URI into (host, port, remote_path).
pub fn parse_sftp_path(path: &str) -> Option<(String, u16, String)> {
    let rest = path.strip_prefix("sftp://")?;
    let (host_port, remote) = rest.split_once('/')?;
    let remote_path = format!("/{}", remote);
    if let Some((host, port_str)) = host_port.rsplit_once(':') {
        let port = port_str.parse::<u16>().ok()?;
        Some((host.to_string(), port, remote_path))
    } else {
        Some((host_port.to_string(), 22, remote_path))
    }
}

/// Strip the `sftp://host:port` prefix, returning just the remote path.
pub fn strip_sftp_prefix(path: &str) -> &str {
    if let Some(rest) = path.strip_prefix("sftp://") {
        if let Some(idx) = rest.find('/') {
            return &rest[idx..];
        }
    }
    path
}
