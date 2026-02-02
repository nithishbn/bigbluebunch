use serde::{Deserialize, Serialize};

/// Represents a scheduled trip with real-time updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripUpdate {
    /// Route ID (e.g., "1" for Route 1, "2" for Route 2)
    pub route_id: String,

    /// Trip ID from GTFS
    pub trip_id: String,

    /// Direction ID (0 or 1, typically outbound/inbound)
    pub direction_id: Option<i32>,

    /// Vehicle serving this trip (if available)
    pub vehicle_id: Option<String>,

    /// List of stop time updates for this trip
    pub stop_time_updates: Vec<StopTimeUpdate>,

    /// Timestamp when this update was generated
    pub timestamp: i64,
}

/// Represents arrival/departure prediction for a specific stop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopTimeUpdate {
    /// Stop sequence number (which stop along the route)
    pub stop_sequence: u32,

    /// Stop ID from GTFS
    pub stop_id: Option<String>,

    /// Arrival time prediction
    pub arrival: Option<StopTimeEvent>,

    /// Departure time prediction
    pub departure: Option<StopTimeEvent>,
}

/// Represents a predicted arrival or departure time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopTimeEvent {
    /// Predicted time (Unix timestamp)
    pub time: Option<i64>,

    /// Delay in seconds (positive = late, negative = early)
    pub delay: Option<i32>,

    /// Uncertainty in seconds
    pub uncertainty: Option<i32>,
}

impl TripUpdate {
    /// Check if this trip matches the given route ID
    pub fn is_route(&self, route_id: &str) -> bool {
        self.route_id == route_id
    }

    /// Format delay in human-readable format
    pub fn format_delay(delay_seconds: i32) -> String {
        let abs_delay = delay_seconds.abs();
        let minutes = abs_delay / 60;
        let seconds = abs_delay % 60;

        if delay_seconds > 0 {
            format!("{}m {}s late", minutes, seconds)
        } else if delay_seconds < 0 {
            format!("{}m {}s early", minutes, seconds)
        } else {
            "on time".to_string()
        }
    }
}

impl std::fmt::Display for TripUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let direction = match self.direction_id {
            Some(0) => "Outbound",
            Some(1) => "Inbound",
            _ => "Unknown",
        };

        write!(
            f,
            "Route {} | Trip {} | {} | {} stops with updates | Vehicle: {}",
            self.route_id,
            self.trip_id,
            direction,
            self.stop_time_updates.len(),
            self.vehicle_id.as_deref().unwrap_or("N/A")
        )
    }
}

impl std::fmt::Display for StopTimeUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "  Stop #{}", self.stop_sequence)?;

        if let Some(stop_id) = &self.stop_id {
            write!(f, " ({})", stop_id)?;
        }

        if let Some(arrival) = &self.arrival {
            if let Some(delay) = arrival.delay {
                write!(f, " | Arrival: {}", TripUpdate::format_delay(delay))?;
            } else if let Some(time) = arrival.time {
                write!(f, " | Arrival: {}", format_timestamp(time))?;
            }
        }

        if let Some(departure) = &self.departure {
            if let Some(delay) = departure.delay {
                write!(f, " | Departure: {}", TripUpdate::format_delay(delay))?;
            } else if let Some(time) = departure.time {
                write!(f, " | Departure: {}", format_timestamp(time))?;
            }
        }

        Ok(())
    }
}

fn format_timestamp(timestamp: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0)
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "invalid time".to_string())
}
