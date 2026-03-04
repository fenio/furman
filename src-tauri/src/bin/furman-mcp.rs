use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let service = app_lib::mcp::FurmanMcp::new();
    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
