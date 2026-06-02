use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::models::PollResult;

pub type Cache = Arc<RwLock<Option<PollResult>>>;

#[derive(Clone)]
struct AppState {
    cache: Cache,
}

#[derive(serde::Deserialize)]
struct DepartureParams {
    stop_ids: Option<String>,
}

/// GET /api/departures?stop_ids=BBB:1234,BBB:5678
/// Returns the latest poll departures, optionally filtered to specific stops.
/// 503 if no poll has completed yet.
async fn get_departures(
    State(state): State<AppState>,
    Query(params): Query<DepartureParams>,
) -> Result<Json<PollResult>, StatusCode> {
    let cache = state.cache.read().await;
    let poll = cache.as_ref().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    match &params.stop_ids {
        Some(ids_str) => {
            let filter: HashSet<&str> = ids_str.split(',').map(|s| s.trim()).collect();
            let filtered: Vec<_> = poll
                .departures
                .iter()
                .filter(|d| filter.contains(d.global_stop_id.as_str()))
                .cloned()
                .collect();
            Ok(Json(PollResult {
                polled_at: poll.polled_at,
                departures: filtered,
            }))
        }
        None => Ok(Json(poll.clone())),
    }
}

/// GET /api/status
async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cache = state.cache.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "last_polled_at": cache.as_ref().map(|p| p.polled_at),
        "departure_count": cache.as_ref().map(|p| p.departures.len()),
        "timestamp": chrono::Utc::now().timestamp(),
    }))
}

pub async fn run_server(addr: &str, cache: Cache) -> Result<()> {
    let state = AppState { cache };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/departures", get(get_departures))
        .route("/api/status", get(get_status))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{}", addr);
    tracing::info!("GET /api/departures?stop_ids=A,B  GET /api/status");

    axum::serve(listener, app).await?;
    Ok(())
}
