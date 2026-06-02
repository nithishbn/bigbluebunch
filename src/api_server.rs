use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::get,
    Router,
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::models::{PollResult, Stop};

pub type Cache = Arc<RwLock<Option<PollResult>>>;

#[derive(Clone)]
struct AppState {
    cache: Cache,
    stops: Arc<Vec<Stop>>,
}

#[derive(serde::Deserialize)]
struct DepartureParams {
    stop_ids: Option<String>,
}

/// GET /api/departures?stop_ids=BBB:1234,BBB:5678
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

/// GET /api/stops — static stop list with coordinates
async fn get_stops(State(state): State<AppState>) -> Json<Vec<Stop>> {
    Json((*state.stops).clone())
}

/// GET /api/status
async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let cache = state.cache.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "last_polled_at": cache.as_ref().map(|p| p.polled_at),
        "departure_count": cache.as_ref().map(|p| p.departures.len()),
        "stop_count": state.stops.len(),
        "timestamp": chrono::Utc::now().timestamp(),
    }))
}

/// GET / — departure map
async fn get_map() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(MAP_HTML),
    )
}

pub async fn run_server(addr: &str, cache: Cache, stops: Arc<Vec<Stop>>) -> Result<()> {
    let state = AppState { cache, stops };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(get_map))
        .route("/api/departures", get(get_departures))
        .route("/api/stops", get(get_stops))
        .route("/api/status", get(get_status))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{}", addr);
    tracing::info!("GET /  GET /api/departures  GET /api/stops  GET /api/status");

    axum::serve(listener, app).await?;
    Ok(())
}

const MAP_HTML: &str = include_str!("../static/map.html");
