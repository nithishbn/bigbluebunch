use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stop {
    pub global_stop_id: String,
    pub stop_name: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Departure {
    pub global_stop_id: String,
    pub global_route_id: String,
    pub route_short_name: String,
    pub headsign: Option<String>,
    pub departure_time: i64,
    pub scheduled_departure_time: i64,
    pub delay_seconds: Option<i32>,
    pub is_real_time: bool,
    pub is_cancelled: bool,
    pub rt_trip_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollResult {
    pub polled_at: i64,
    pub departures: Vec<Departure>,
}
