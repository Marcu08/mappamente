//! Console entry point for the Mappamente MCP server.
//!
//! The binary speaks MCP over stdin/stdout. Nothing but protocol messages is
//! written to stdout; diagnostics go to stderr.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use mappamente_mcp::{MapStore, MappamenteServer};
use rmcp::ServiceExt;

#[derive(Debug, Parser)]
#[command(
    name = "mappamente-mcp",
    version,
    about = "stdio Model Context Protocol server for Mappamente mind maps"
)]
struct Cli {
    /// Directory that holds the maps; map names cannot escape it
    #[arg(long, value_name = "DIR")]
    dir: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = MapStore::new(&cli.dir)
        .with_context(|| format!("cannot use {} as the maps directory", cli.dir.display()))?;
    eprintln!(
        "mappamente-mcp: serving {} over stdio",
        store.root().display()
    );

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?
        .block_on(serve(store))
}

async fn serve(store: MapStore) -> Result<()> {
    let server = MappamenteServer::new(store);
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .context("MCP initialization failed")?;
    eprintln!("mappamente-mcp: client connected");
    running.waiting().await.context("MCP server stopped")?;
    eprintln!("mappamente-mcp: client disconnected");
    Ok(())
}
