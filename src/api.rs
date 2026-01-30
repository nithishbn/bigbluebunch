use anyhow::{Context, Result};
use bytes::Bytes;
use prost::Message;
use crate::models::{BusObservation, PollStats};

// Include the generated protobuf code
pub mod gtfs_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}

const VEHICLE_POSITIONS_URL: &str = "http://gtfs.bigbluebus.com/vehiclepositions.bin";

/// GTFS-RT API client for Big Blue Bus
pub struct GtfsClient {
    client: reqwest::Client,
}

impl GtfsClient {
    /// Create a new GTFS-RT client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Fetch vehicle positions from the GTFS-RT API
    pub async fn fetch_vehicle_positions(&self) -> Result<Bytes> {
        tracing::debug!(url = VEHICLE_POSITIONS_URL, "Fetching vehicle positions");

        let response = self.client
            .get(VEHICLE_POSITIONS_URL)
            .send()
            .await
            .context("Failed to fetch vehicle positions")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error status: {}", response.status());
        }

        let bytes = response.bytes().await
            .context("Failed to read response body")?;

        tracing::debug!(bytes = bytes.len(), "Received data from API");
        Ok(bytes)
    }

    /// Parse GTFS-RT feed and extract bus observations
    pub fn parse_feed(&self, data: &[u8]) -> Result<Vec<BusObservation>> {
        use gtfs_realtime::FeedMessage;

        let feed = FeedMessage::decode(data)
            .context("Failed to decode protobuf message")?;

        tracing::debug!(entities = feed.entity.len(), "Decoded protobuf feed");

        let mut observations = Vec::new();

        for entity in feed.entity {
            // Only process entities that have vehicle position data
            if let Some(vehicle) = entity.vehicle {
                // Extract trip information
                let (route_id, trip_id, direction_id) = if let Some(trip) = vehicle.trip {
                    (
                        trip.route_id.clone(),
                        trip.trip_id.clone(),
                        trip.direction_id,
                    )
                } else {
                    continue; // Skip if no trip info
                };

                // Extract vehicle ID
                let vehicle_id = vehicle.vehicle
                    .and_then(|v| v.id)
                    .unwrap_or_else(|| "unknown".to_string());

                // Extract position
                if let Some(position) = vehicle.position {
                    let latitude = position.latitude as f64;
                    let longitude = position.longitude as f64;

                    // Get timestamp (prefer vehicle timestamp, fallback to feed header)
                    let timestamp = vehicle.timestamp.unwrap_or_else(|| {
                        feed.header.timestamp.unwrap_or(0)
                    }) as i64;

                    let mut obs = BusObservation::new(
                        timestamp,
                        vehicle_id,
                        route_id.unwrap_or_else(|| "unknown".to_string()),
                        latitude,
                        longitude,
                    );

                    // Add optional fields
                    obs.trip_id = trip_id;
                    obs.direction_id = direction_id.map(|d| d as i32);
                    obs.current_stop_sequence = vehicle.current_stop_sequence.map(|s| s as i32);
                    obs.speed = position.speed;
                    obs.bearing = position.bearing;

                    observations.push(obs);
                }
            }
        }

        tracing::info!(count = observations.len(), "Parsed vehicle observations");
        Ok(observations)
    }

    /// Poll the API and return bus observations for Route 1
    pub async fn poll_route_1(&self) -> Result<(Vec<BusObservation>, PollStats)> {
        let data = self.fetch_vehicle_positions().await?;
        let all_observations = self.parse_feed(&data)?;

        let total_vehicles = all_observations.len();

        // Filter for Route 1
        let route_1: Vec<_> = all_observations.into_iter()
            // .filter(|obs| obs.is_route_1())
            .collect();

        let route_1_count = route_1.len();

        let stats = PollStats {
            total_vehicles,
            route_1_vehicles: route_1_count,
            timestamp: chrono::Utc::now().timestamp(),
        };

        tracing::debug!(
            total = total_vehicles,
            route_1 = route_1_count,
            "Filtered vehicles"
        );

        Ok((route_1, stats))
    }
}

impl Default for GtfsClient {
    fn default() -> Self {
        Self::new()
    }
}
