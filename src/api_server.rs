use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use crate::{
    api::TransitClient,
    db::Database,
    models::{PollResult, Stop},
    poll_once,
};

pub type Cache = Arc<RwLock<Option<PollResult>>>;

#[derive(Clone)]
struct AppState {
    cache: Cache,
    stops: Arc<Vec<Stop>>,
    client: Arc<TransitClient>,
    db: Arc<Database>,
    stop_ids: Arc<Vec<String>>,
    chunks_per_poll: usize,
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

/// GET /api/quota — API call counts derived from departure_log
async fn get_quota(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let (total_polls, today_polls) = state
        .db
        .count_polls()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let c = state.chunks_per_poll as i64;
    Ok(Json(serde_json::json!({
        "total_polls": total_polls,
        "today_polls": today_polls,
        "chunks_per_poll": c,
        "total_api_calls": total_polls * c,
        "today_api_calls": today_polls * c,
    })))
}

/// POST /api/refresh — force an immediate poll regardless of time window
async fn post_refresh(
    State(state): State<AppState>,
) -> Result<Json<PollResult>, StatusCode> {
    match poll_once(&state.client, &state.db, &state.stop_ids, false).await {
        Some(result) => {
            *state.cache.write().await = Some(result.clone());
            Ok(Json(result))
        }
        None => Err(StatusCode::BAD_GATEWAY),
    }
}

/// GET / — departure map
async fn get_map() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(MAP_HTML),
    )
}

pub async fn run_server(
    addr: &str,
    cache: Cache,
    stops: Arc<Vec<Stop>>,
    client: Arc<TransitClient>,
    db: Arc<Database>,
    stop_ids: Arc<Vec<String>>,
    chunks_per_poll: usize,
) -> Result<()> {
    let state = AppState { cache, stops, client, db, stop_ids, chunks_per_poll };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(get_map))
        .route("/api/departures", get(get_departures))
        .route("/api/stops", get(get_stops))
        .route("/api/status", get(get_status))
        .route("/api/quota", get(get_quota))
        .route("/api/refresh", post(post_refresh))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{}", addr);
    tracing::info!("GET /  GET /api/departures  GET /api/stops  GET /api/status  GET /api/quota  POST /api/refresh");

    axum::serve(listener, app).await?;
    Ok(())
}

const MAP_HTML: &str = include_str!("../static/map.html");
