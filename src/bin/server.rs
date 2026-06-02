use anyhow::{Context, Result};
use bigbluebunch::{api::TransitClient, api_server, db::Database, poll_once};
use chrono::{Datelike, Local, Timelike, Weekday};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// 15 min → 24 polls/day × 2 calls/poll × 22 weekdays = ~1056 calls/month
const POLL_INTERVAL_SECS: u64 = 900;

fn is_active_window() -> bool {
    let now = Local::now();
    match now.weekday() {
        Weekday::Sat | Weekday::Sun => return false,
        _ => {}
    }
    let h = now.hour();
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

    // Routes for full stop bootstrap — all stops on these routes go into the stops table
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

    // Routes used only to resolve stop metadata for EXTRA_STOP_IDS.
    // All stops on these routes are fetched but only those matching EXTRA_STOP_IDS are kept.
    let extra_route_ids: Vec<String> = std::env::var("EXTRA_ROUTE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Specific stop IDs to watch — must overlap with stops seeded by ROUTE_IDS or EXTRA_ROUTE_IDS
    let extra_stop_ids: HashSet<String> = std::env::var("EXTRA_STOP_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);

    let database_url = std::env::var("DATABASE_URL")
        .context("DATABASE_URL must be set in .env")?;

    let client = Arc::new(TransitClient::from_env());
    let db = Arc::new(Database::new(&database_url).await?);
    let cache: api_server::Cache = Arc::new(RwLock::new(None));

    // ── Full route bootstrap (runs once, when stops table is empty) ──────────
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
        tokio::time::sleep(Duration::from_secs(13)).await;
    }

    // ── Extra route bootstrap (metadata only, filtered to EXTRA_STOP_IDS) ───
    // Runs whenever any EXTRA_STOP_IDS are missing from the stops table.
    if !extra_route_ids.is_empty() && !extra_stop_ids.is_empty() {
        let in_db: HashSet<String> = db.get_all_stop_ids().await?.into_iter().collect();
        let missing: HashSet<&String> = extra_stop_ids.iter().filter(|id| !in_db.contains(*id)).collect();

        if !missing.is_empty() {
            tracing::info!(count = missing.len(), "Bootstrapping extra stop metadata from EXTRA_ROUTE_IDS");
            for (i, route_id) in extra_route_ids.iter().enumerate() {
                if i > 0 {
                    tokio::time::sleep(Duration::from_secs(13)).await;
                }
                match client.fetch_route_stops(route_id).await {
                    Ok(stops) => {
                        let filtered: Vec<_> = stops.into_iter()
                            .filter(|s| missing.contains(&s.global_stop_id))
                            .collect();
                        tracing::info!(route = %route_id, count = filtered.len(), "Upserted extra stops");
                        if !filtered.is_empty() {
                            db.upsert_stops(&filtered).await?;
                        }
                    }
                    Err(e) => {
                        tracing::error!(route = %route_id, error = %e, "Failed to fetch extra route stops");
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(13)).await;
        }
    }

    // Poll list is simply everything in the stops table — both full-route stops and extra stops
    let stop_ids = db.get_all_stop_ids().await?;

    if stop_ids.is_empty() {
        anyhow::bail!("No stops configured — check ROUTE_IDS and EXTRA_ROUTE_IDS");
    }

    // Load full stop metadata (with coordinates) for the map
    let stops = Arc::new(db.get_all_stops().await?);

    tracing::info!(
        stops = stop_ids.len(),
        chunks = stop_ids.chunks(100).count(),
        interval_secs = POLL_INTERVAL_SECS,
        "Ready — polling active weekdays 7–10am and 4–7pm"
    );

    let chunks_per_poll = stop_ids.chunks(100).count();

    // ── Departures poll task ─────────────────────────────────────────────────
    {
        let client_poll = Arc::clone(&client);
        let db_poll = Arc::clone(&db);
        let cache_poll = Arc::clone(&cache);
        let stop_ids_poll = stop_ids.clone();

        tokio::spawn(async move {
            if let Some(result) = poll_once(&client_poll, &db_poll, &stop_ids_poll, true).await {
                *cache_poll.write().await = Some(result);
            }

            let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if !is_active_window() {
                    continue;
                }
                if let Some(result) = poll_once(&client_poll, &db_poll, &stop_ids_poll, true).await {
                    *cache_poll.write().await = Some(result);
                }
            }
        });
    }

    api_server::run_server(&addr, cache, stops, client, db, Arc::new(stop_ids), chunks_per_poll).await?;
    Ok(())
}
