use serde::{Deserialize, Serialize};

/// Represents a single bus observation with its position and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusObservation {
    /// Unix timestamp when this observation was recorded
    pub timestamp: i64,

    /// Unique vehicle identifier (bus number)
    pub vehicle_id: String,

    /// Route ID (e.g., "1" for Route 1)
    pub route_id: String,

    /// Trip ID from GTFS
    pub trip_id: Option<String>,

    /// Direction ID (0 or 1, typically inbound/outbound)
    pub direction_id: Option<i32>,

    /// Current latitude
    pub latitude: f64,

    /// Current longitude
    pub longitude: f64,

    /// Current stop sequence number (which stop the bus is at/approaching)
    pub current_stop_sequence: Option<i32>,

    /// Speed in meters per second
    pub speed: Option<f32>,

    /// Bearing/heading in degrees
    pub bearing: Option<f32>,
}

impl BusObservation {
    /// Create a new BusObservation from GTFS-RT vehicle position data
    pub fn new(
        timestamp: i64,
        vehicle_id: String,
        route_id: String,
        latitude: f64,
        longitude: f64,
    ) -> Self {
        Self {
            timestamp,
            vehicle_id,
            route_id,
            trip_id: None,
            direction_id: None,
            latitude,
            longitude,
            current_stop_sequence: None,
            speed: None,
            bearing: None,
        }
    }

    /// Check if this observation is for Route 1
    pub fn is_route_1(&self) -> bool {
        self.route_id == "1"
    }
}

/// Statistics about a polling session
#[derive(Debug, Default)]
pub struct PollStats {
    pub total_vehicles: usize,
    pub route_1_vehicles: usize,
    pub timestamp: i64,
}

impl std::fmt::Display for BusObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bus {} on Route {} at ({:.6}, {:.6}) [{}]",
            self.vehicle_id,
            self.route_id,
            self.latitude,
            self.longitude,
            chrono::DateTime::<chrono::Utc>::from_timestamp(self.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "invalid timestamp".to_string())
        )
    }
}
