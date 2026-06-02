use anyhow::Result;
use bigbluebunch::{api::TransitClient, api_server, db::Database, models::PollResult};
use chrono::{Local, Timelike};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// 15 minutes → 24 polls/day × 2 calls/poll = 48 calls/day (~1440/month)
const POLL_INTERVAL_SECS: u64 = 900;

fn is_active_window() -> bool {
    let h = Local::now().hour();
    (h >= 7 && h < 10) || (h >= 16 && h < 19)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match dotenvy::dotenv() {
        Ok(path) => eprintln!(".env loaded from {:?}", path),
        Err(e) => eprintln!(".env not found: {}", e),
    }

    tracing::info!("Big Blue Bus Tracker starting");

    let route_ids: Vec<String> = std::env::var("ROUTE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if route_ids.is_empty() {
        anyhow::bail!(
            "ROUTE_IDS must be set in .env\n\
             Run: cargo run -- --discover   to find route IDs near UCLA"
        );
    }

    // Extra stop IDs (specific stops for routes not fully bootstrapped, e.g. Route 6, 17)
    let extra_stop_ids: Vec<String> = std::env::var("EXTRA_STOP_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if !extra_stop_ids.is_empty() {
        tracing::info!(count = extra_stop_ids.len(), "Extra stop IDs loaded from EXTRA_STOP_IDS");
    }

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let client = Arc::new(TransitClient::from_env());
    let db = Arc::new(Database::new("bus_tracking.db").await?);
    let cache: api_server::Cache = Arc::new(RwLock::new(None));

    // Bootstrap: seed stops from route_details if table is empty.
    // Each route costs 1 API call, runs once ever.
    if !db.stops_initialized().await? {
        tracing::info!(routes = route_ids.len(), "Bootstrapping stops from route_details");
        for (i, route_id) in route_ids.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(Duration::from_secs(13)).await;
            }
            match client.fetch_route_stops(route_id).await {
                Ok(stops) => {
                    tracing::info!(route = %route_id, count = stops.len(), "Fetched stops");
                    db.upsert_stops(&stops).await?;
                }
                Err(e) => {
                    tracing::error!(route = %route_id, error = %e, "Failed to fetch stops");
                }
            }
        }
        // Wait before the first poll so we don't immediately spike over 5/min
        tokio::time::sleep(Duration::from_secs(13)).await;
    }

    // Merge DB stop IDs with extra stop IDs, deduplicating
    let mut stop_ids = db.get_all_stop_ids().await?;
    let existing: std::collections::HashSet<_> = stop_ids.iter().cloned().collect();
    for id in &extra_stop_ids {
        if !existing.contains(id) {
            stop_ids.push(id.clone());
        }
    }

    if stop_ids.is_empty() {
        anyhow::bail!("No stops configured — check ROUTE_IDS and EXTRA_STOP_IDS");
    }

    tracing::info!(
        stops = stop_ids.len(),
        chunks = stop_ids.chunks(100).count(),
        interval_secs = POLL_INTERVAL_SECS,
        "Ready — polling active 7–10am and 4–7pm"
    );

    let client_poll = Arc::clone(&client);
    let db_poll = Arc::clone(&db);
    let cache_poll = Arc::clone(&cache);

    tokio::spawn(async move {
        // Poll immediately on startup so the cache isn't empty
        do_poll(&client_poll, &db_poll, &cache_poll, &stop_ids).await;

        let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if !is_active_window() {
                continue;
            }
            do_poll(&client_poll, &db_poll, &cache_poll, &stop_ids).await;
        }
    });

    api_server::run_server(&addr, cache).await?;
    Ok(())
}

async fn do_poll(
    client: &TransitClient,
    db: &Database,
    cache: &api_server::Cache,
    stop_ids: &[String],
) {
    let polled_at = chrono::Utc::now().timestamp();
    let mut all_departures = Vec::new();

    for (i, chunk) in stop_ids.chunks(100).enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(13)).await;
        }
        match client.fetch_stop_departures(chunk).await {
            Ok(deps) => all_departures.extend(deps),
            Err(e) => {
                tracing::error!(error = %e, "Poll failed");
                return;
            }
        }
    }

    let count = all_departures.len();

    if let Err(e) = db.insert_departure_log(polled_at, &all_departures).await {
        tracing::error!(error = %e, "Failed to persist departures");
    }

    *cache.write().await = Some(PollResult {
        polled_at,
        departures: all_departures,
    });

    tracing::info!(departures = count, "Poll complete");
}
