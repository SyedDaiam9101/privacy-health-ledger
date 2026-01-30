mod api;
mod dataset;
mod db;
mod errors;
mod models;
mod state;

use crate::errors::ApiError;
use crate::state::AppState;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), ApiError> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    // Store local state under backend/data (ignored by git).
    let data_dir = PathBuf::from("data");
    std::fs::create_dir_all(&data_dir).map_err(|_| ApiError::Internal)?;

    let db_path = data_dir.join("ledger.sqlite");
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());

    let db = db::connect(&db_url).await?;
    db::init_schema(&db).await?;

    let state = AppState::new(db, data_dir);

    let app = api::router(state);

    let addr = std::env::var("BACKEND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|_| ApiError::Internal)?;

    tracing::info!(%addr, "backend listening");

    axum::serve(listener, app).await.map_err(|_| ApiError::Internal)?;

    Ok(())
}
