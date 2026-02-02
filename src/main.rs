mod api;
mod models;

use anyhow::Result;
use api::GtfsClient;
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

    tracing::info!("Big Blue Bus Trip Updates Monitor");
    tracing::info!("Monitoring Route 1 and Route 2");
    tracing::info!("");

    // Initialize GTFS-RT client
    let client = GtfsClient::new();
    tracing::info!("GTFS-RT client ready");
    tracing::info!("");

    // Polling interval: 30 seconds
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    let mut poll_count = 0;

    tracing::info!("Starting polling loop (30 second interval)");
    tracing::info!("=====================================");
    tracing::info!("");

    loop {
        interval.tick().await;
        poll_count += 1;

        println!("\n┌─ Poll #{} ─────────────────────────────────────", poll_count);
        println!("│ Time: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!("└────────────────────────────────────────────────────");

        match client.poll_routes(&["1", "2"]).await {
            Ok(trip_updates) => {
                let route_1_count = trip_updates.iter().filter(|t| t.is_route("1")).count();
                let route_2_count = trip_updates.iter().filter(|t| t.is_route("2")).count();

                println!("\n📊 Summary: {} total trips ({} Route 1, {} Route 2)",
                    trip_updates.len(), route_1_count, route_2_count);

                if trip_updates.is_empty() {
                    println!("\n⚠️  No active trips found for Route 1 or Route 2");
                } else {
                    // Group by route for display
                    let route_1: Vec<_> = trip_updates.iter().filter(|t| t.is_route("1")).collect();
                    let route_2: Vec<_> = trip_updates.iter().filter(|t| t.is_route("2")).collect();

                    // Display Route 1
                    if !route_1.is_empty() {
                        println!("\n🚌 Route 1 ({} trips):", route_1.len());
                        println!("─────────────────────────────────────────────────");
                        for trip in route_1 {
                            println!("{}", trip);
                            // Show first 3 stops with updates as example
                            for stop in trip.stop_time_updates.iter().take(3) {
                                println!("{}", stop);
                            }
                            if trip.stop_time_updates.len() > 3 {
                                println!("  ... and {} more stops", trip.stop_time_updates.len() - 3);
                            }
                            println!();
                        }
                    }

                    // Display Route 2
                    if !route_2.is_empty() {
                        println!("\n🚌 Route 2 ({} trips):", route_2.len());
                        println!("─────────────────────────────────────────────────");
                        for trip in route_2 {
                            println!("{}", trip);
                            // Show first 3 stops with updates as example
                            for stop in trip.stop_time_updates.iter().take(3) {
                                println!("{}", stop);
                            }
                            if trip.stop_time_updates.len() > 3 {
                                println!("  ... and {} more stops", trip.stop_time_updates.len() - 3);
                            }
                            println!();
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("\n❌ Poll failed: {}", e);
                eprintln!("⏳ Will retry on next interval");
            }
        }

        println!("\n════════════════════════════════════════════════════\n");
    }
}
