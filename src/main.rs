mod api;
mod db;
mod models;

use anyhow::Result;
use api::GtfsClient;
use db::Database;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    tracing::info!("Big Blue Bus Route 1 Bunching Tracker");
    tracing::info!("Starting polling service...");

    // Initialize database
    let db = Database::new("bus_tracking.db").await?;
    tracing::info!("Database initialized");

    // Initialize GTFS-RT client
    let client = GtfsClient::new();
    tracing::info!("GTFS-RT client ready");

    // Print initial stats
    let total_count = db.count_observations().await?;
    let route_1_count = db.count_route_1_observations().await?;
    tracing::info!(
        total_observations = total_count,
        route_1_observations = route_1_count,
        "Database initialized with existing data"
    );

    // Polling interval: 60 seconds
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    let mut poll_count = 0;

    tracing::info!("Starting polling loop (60 second interval)");
    tracing::info!("");

    loop {
        interval.tick().await;
        poll_count += 1;

        tracing::info!(poll_number = poll_count, "Starting poll");

        match client.poll_route_1().await {
            Ok((observations, stats)) => {
                tracing::info!(
                    total_vehicles = stats.total_vehicles,
                    route_1_vehicles = stats.route_1_vehicles,
                    "Poll complete"
                );

                // Display each bus
                for obs in &observations {
                    tracing::info!(
                        vehicle_id = %obs.vehicle_id,
                        route_id = %obs.route_id,
                        lat = obs.latitude,
                        lon = obs.longitude,
                        "Bus position"
                    );
                }

                // Save to database
                if !observations.is_empty() {
                    match db.insert_observations(&observations).await {
                        Ok(count) => {
                            tracing::info!(saved_count = count, "Saved observations to database");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to save observations");
                        }
                    }
                } else {
                    tracing::info!("No buses currently active");
                }

                // Update stats
                let total = db.count_observations().await.unwrap_or(0);
                let route_1 = db.count_route_1_observations().await.unwrap_or(0);
                tracing::info!(
                    total_observations = total,
                    route_1_observations = route_1,
                    "Database stats"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Poll failed");
                tracing::warn!("Will retry on next interval");
            }
        }

        tracing::info!("");
    }
}
