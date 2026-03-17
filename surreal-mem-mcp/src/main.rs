mod ast;
mod embeddings;
mod http_server;
mod mcp_types;
mod resources;
mod surreal_client;
mod tools;
pub mod security;

use std::env;
use std::sync::Arc;
use surreal_client::SurrealClient;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Try loading from project root first, then fallback to global ~/.surreal-mem-mcp/.env
    let mut home = home::home_dir().unwrap_or_default();
    if dotenvy::dotenv().is_err() && !home.as_os_str().is_empty() {
        home.push(".surreal-mem-mcp");
        home.push(".env");
        let _ = dotenvy::from_path(home);
    }

    // Determine database path (default: local memory_store)
    let db_path = env::var("SURREAL_DB_PATH").unwrap_or_else(|_| "memory_store".to_string());

    // Initialize the SurrealDB client
    let surreal_client = Arc::new(SurrealClient::connect(db_path).await?);

    let port = env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);

    println!("Starting Surreal-Mem-MCP Server (SSE) on http://{}", addr);

    let app = http_server::create_router(surreal_client).layer(CorsLayer::permissive());
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
