pub mod api;
pub mod api_server;
pub mod db;
pub mod models;

use std::time::Duration;
use api::TransitClient;
use db::Database;
use models::PollResult;

pub async fn poll_once(
    client: &TransitClient,
    db: &Database,
    stop_ids: &[String],
    rate_limit: bool,
) -> Option<PollResult> {
    let polled_at = chrono::Utc::now().timestamp();
    let mut all_departures = Vec::new();

    for (i, chunk) in stop_ids.chunks(100).enumerate() {
        if i > 0 && rate_limit {
            tokio::time::sleep(Duration::from_secs(13)).await;
        }
        match client.fetch_stop_departures(chunk).await {
            Ok(deps) => all_departures.extend(deps),
            Err(e) => {
                tracing::error!(error = %e, "Departures poll failed");
                return None;
            }
        }
    }

    let count = all_departures.len();
    if let Err(e) = db.insert_departure_log(polled_at, &all_departures).await {
        tracing::error!(error = %e, "Failed to persist departures");
    }

    tracing::info!(departures = count, "Poll complete");
    Some(PollResult { polled_at, departures: all_departures })
}
