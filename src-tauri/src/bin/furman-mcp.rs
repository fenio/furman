use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use std::io::IsTerminal;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    eprintln!("furman-mcp {VERSION} — MCP server for S3 and SFTP operations");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("    furman-mcp              Start the MCP server (stdio transport)");
    eprintln!("    furman-mcp --help       Show this help message");
    eprintln!("    furman-mcp --version    Show version");
    eprintln!();
    eprintln!("This is an MCP (Model Context Protocol) server that exposes Furman's");
    eprintln!("S3 and SFTP capabilities as tools. It communicates via JSON-RPC over");
    eprintln!("stdin/stdout and is designed to be launched by an MCP client such as");
    eprintln!("Claude Desktop.");
    eprintln!();
    eprintln!("CONFIGURATION (Claude Desktop):");
    eprintln!("    Add to ~/Library/Application Support/Claude/claude_desktop_config.json:");
    eprintln!();
    eprintln!(r#"    "furman": {{"#);
    eprintln!(r#"      "command": "/Applications/Furman.app/Contents/MacOS/furman-mcp""#);
    eprintln!(r#"    }}"#);
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("furman-mcp {VERSION}");
        return Ok(());
    }

    if std::io::stdin().is_terminal() {
        eprintln!("furman-mcp {VERSION} — MCP server for S3 and SFTP operations");
        eprintln!("This binary communicates via JSON-RPC over stdin/stdout.");
        eprintln!("Run with --help for more information.");
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let service = app_lib::mcp::FurmanMcp::new();
    let server = service.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
