use anyhow::Result;
use bigbluebunch::api::TransitClient;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    // --discover: find BBB Route 1's global_route_id near UCLA.
    if args.contains(&"--discover".to_string()) {
        let api_key = std::env::var("TRANSIT_API_KEY").expect("TRANSIT_API_KEY must be set");
        TransitClient::new(api_key).discover_route_id().await?;
        return Ok(());
    }

    // --resolve-stops <lat> <lon>: print stop IDs near a coordinate.
    // Use this to populate WATCH_POINT_N_STOP_IDS in .env.
    if let Some(pos) = args.iter().position(|a| a == "--resolve-stops") {
        let lat = args
            .get(pos + 1)
            .expect("--resolve-stops requires lat")
            .parse::<f64>()
            .expect("invalid lat");
        let lon = args
            .get(pos + 2)
            .expect("--resolve-stops requires lon")
            .parse::<f64>()
            .expect("invalid lon");

        let api_key = std::env::var("TRANSIT_API_KEY").expect("TRANSIT_API_KEY must be set");
        let stops = TransitClient::new(api_key).resolve_stops(lat, lon).await?;

        println!("{:<32} {:<8} {}", "stop_id", "code", "name");
        println!("{}", "-".repeat(70));
        for stop in &stops {
            println!(
                "{:<32} {:<8} {}  ({:.0}m)",
                stop.global_stop_id,
                stop.stop_code.as_deref().unwrap_or("?"),
                stop.stop_name,
                stop.distance.unwrap_or(0.0),
            );
        }

        return Ok(());
    }

    eprintln!("Usage:");
    eprintln!("  cargo run -- --discover                    find BBB route IDs near UCLA");
    eprintln!("  cargo run -- --resolve-stops <lat> <lon>   find stop IDs near a coordinate");
    eprintln!("  cargo run --bin server                      start the collection server");

    Ok(())
}
