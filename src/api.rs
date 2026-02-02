use anyhow::{Context, Result};
use bytes::Bytes;
use prost::Message;
use crate::models::{TripUpdate, StopTimeUpdate, StopTimeEvent};

// Include the generated protobuf code
pub mod gtfs_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}

const TRIP_UPDATES_URL: &str = "http://gtfs.bigbluebus.com/tripupdates.bin";

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

    /// Fetch trip updates from the GTFS-RT API
    pub async fn fetch_trip_updates(&self) -> Result<Bytes> {
        tracing::debug!(url = TRIP_UPDATES_URL, "Fetching trip updates");

        let response = self.client
            .get(TRIP_UPDATES_URL)
            .send()
            .await
            .context("Failed to fetch trip updates")?;

        if !response.status().is_success() {
            anyhow::bail!("API returned error status: {}", response.status());
        }

        let bytes = response.bytes().await
            .context("Failed to read response body")?;

        tracing::debug!(bytes = bytes.len(), "Received data from API");
        Ok(bytes)
    }

    /// Parse GTFS-RT feed and extract trip updates
    pub fn parse_feed(&self, data: &[u8]) -> Result<Vec<TripUpdate>> {
        use gtfs_realtime::FeedMessage;

        let feed = FeedMessage::decode(data)
            .context("Failed to decode protobuf message")?;

        tracing::debug!(entities = feed.entity.len(), "Decoded protobuf feed");

        let mut trip_updates = Vec::new();
        let feed_timestamp = feed.header.timestamp.unwrap_or(0) as i64;

        for entity in feed.entity {
            // Only process entities that have trip update data
            if let Some(trip_update) = entity.trip_update {
                // Extract trip information (trip is a required field)
                let trip_descriptor = &trip_update.trip;

                let route_id = trip_descriptor.route_id.clone().unwrap_or_else(|| "unknown".to_string());
                let trip_id = trip_descriptor.trip_id.clone().unwrap_or_else(|| "unknown".to_string());
                let direction_id = trip_descriptor.direction_id.map(|d| d as i32);

                // Extract vehicle ID if available
                let vehicle_id = trip_update.vehicle
                    .and_then(|v| v.id);

                // Extract stop time updates
                let mut stop_time_updates = Vec::new();
                for stu in trip_update.stop_time_update {
                    let stop_sequence = stu.stop_sequence.unwrap_or(0);
                    let stop_id = stu.stop_id;

                    let arrival = stu.arrival.map(|a| StopTimeEvent {
                        time: a.time,
                        delay: a.delay,
                        uncertainty: a.uncertainty,
                    });

                    let departure = stu.departure.map(|d| StopTimeEvent {
                        time: d.time,
                        delay: d.delay,
                        uncertainty: d.uncertainty,
                    });

                    stop_time_updates.push(StopTimeUpdate {
                        stop_sequence,
                        stop_id,
                        arrival,
                        departure,
                    });
                }

                // Get timestamp (prefer trip update timestamp, fallback to feed header)
                let timestamp = trip_update.timestamp.map(|t| t as i64).unwrap_or(feed_timestamp);

                trip_updates.push(TripUpdate {
                    route_id,
                    trip_id,
                    direction_id,
                    vehicle_id,
                    stop_time_updates,
                    timestamp,
                });
            }
        }

        tracing::info!(count = trip_updates.len(), "Parsed trip updates");
        Ok(trip_updates)
    }

    /// Poll the API and return all trip updates
    pub async fn poll_trip_updates(&self) -> Result<Vec<TripUpdate>> {
        let data = self.fetch_trip_updates().await?;
        let trip_updates = self.parse_feed(&data)?;

        tracing::debug!(
            total = trip_updates.len(),
            "Fetched trip updates"
        );

        Ok(trip_updates)
    }

    /// Poll the API and return trip updates filtered by route IDs
    pub async fn poll_routes(&self, route_ids: &[&str]) -> Result<Vec<TripUpdate>> {
        let all_updates = self.poll_trip_updates().await?;

        let filtered: Vec<TripUpdate> = all_updates
            .into_iter()
            .filter(|update| route_ids.iter().any(|&id| update.is_route(id)))
            .collect();

        tracing::info!(
            total = filtered.len(),
            routes = ?route_ids,
            "Filtered trip updates"
        );

        Ok(filtered)
    }
}

impl Default for GtfsClient {
    fn default() -> Self {
        Self::new()
    }
}
