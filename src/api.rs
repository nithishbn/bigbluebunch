use crate::models::{Departure, Stop};
use anyhow::{Context, Result};
use serde::Deserialize;

const TRANSIT_API_BASE: &str = "https://external.transitapp.com";
const DISCOVERY_LAT: f64 = 34.04363632;
const DISCOVERY_LON: f64 = -118.45709929;

pub struct TransitClient {
    client: reqwest::Client,
    api_key: String,
}

// --- route_details response ---

#[derive(Deserialize)]
struct RouteDetailsResponse {
    itineraries: Vec<ItineraryDetail>,
}

#[derive(Deserialize)]
struct ItineraryDetail {
    #[serde(default)]
    stops: Vec<RouteStop>,
}

#[derive(Deserialize)]
struct RouteStop {
    global_stop_id: String,
    stop_name: String,
    stop_lat: f64,
    stop_lon: f64,
}

// --- stop_departures response ---

#[derive(Deserialize)]
struct StopDeparturesResponse {
    route_departures: Vec<StopRouteDeparture>,
}

#[derive(Deserialize)]
struct StopRouteDeparture {
    global_route_id: String,
    route_short_name: String,
    global_stop_id: String,
    #[serde(default)]
    merged_itineraries: Vec<MergedItinerary>,
}

#[derive(Deserialize)]
struct MergedItinerary {
    #[serde(default)]
    itineraries: Vec<RouteItinerary>,
    #[serde(default)]
    schedule_items: Vec<ScheduleItem>,
}

#[derive(Deserialize)]
struct RouteItinerary {
    headsign: Option<String>,
    merged_headsign: Option<String>,
}

#[derive(Deserialize)]
struct ScheduleItem {
    departure_time: i64,
    scheduled_departure_time: Option<i64>,
    #[serde(default)]
    is_real_time: bool,
    #[serde(default)]
    is_cancelled: bool,
    rt_trip_id: Option<String>,
}

// --- nearby_stops response (resolve-stops discovery) ---

#[derive(Deserialize)]
pub struct NearbyStopsResponse {
    pub stops: Vec<NearbyStop>,
}

#[derive(Deserialize)]
pub struct NearbyStop {
    pub global_stop_id: String,
    pub stop_name: String,
    pub stop_code: Option<String>,
    pub distance: Option<f64>,
}

// --- nearby_routes response (route discovery) ---

#[derive(Deserialize)]
struct DiscoverRoutesResponse {
    nearby_routes: Vec<DiscoverRoute>,
}

#[derive(Deserialize)]
struct DiscoverRoute {
    global_route_id: String,
    route_short_name: String,
    route_network_name: Option<String>,
}

impl TransitClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            api_key,
        }
    }

    pub fn from_env() -> Self {
        let api_key = std::env::var("TRANSIT_API_KEY").expect("TRANSIT_API_KEY must be set");
        Self::new(api_key)
    }

    /// Bootstrap: fetch all stops for a route from route_details.
    /// Deduplicates across itineraries/directions.
    pub async fn fetch_route_stops(&self, global_route_id: &str) -> Result<Vec<Stop>> {
        let response = self
            .client
            .get(format!("{}/v4/public/route_details", TRANSIT_API_BASE))
            .header("apiKey", &self.api_key)
            .query(&[("global_route_id", global_route_id)])
            .send()
            .await
            .context("Failed to call route_details")?;

        if !response.status().is_success() {
            anyhow::bail!("route_details returned {}", response.status());
        }

        let body: RouteDetailsResponse = response
            .json()
            .await
            .context("Failed to parse route_details response")?;

        let mut seen = std::collections::HashSet::new();
        let mut stops = Vec::new();

        for itinerary in body.itineraries {
            for s in itinerary.stops {
                if seen.insert(s.global_stop_id.clone()) {
                    stops.push(Stop {
                        global_stop_id: s.global_stop_id,
                        stop_name: s.stop_name,
                        lat: s.stop_lat,
                        lon: s.stop_lon,
                    });
                }
            }
        }

        Ok(stops)
    }

    /// Poll: fetch upcoming real-time departures for a batch of stop IDs (max 100 per call).
    pub async fn fetch_stop_departures(&self, stop_ids: &[String]) -> Result<Vec<Departure>> {
        let stop_ids_param = stop_ids.join(",");

        let response = self
            .client
            .get(format!("{}/v4/public/stop_departures", TRANSIT_API_BASE))
            .header("apiKey", &self.api_key)
            .query(&[
                ("global_stop_ids", stop_ids_param.as_str()),
                ("should_update_realtime", "true"),
                ("max_num_departures", "10"),
            ])
            .send()
            .await
            .context("Failed to call stop_departures")?;

        if !response.status().is_success() {
            anyhow::bail!("stop_departures returned {}", response.status());
        }

        let body: StopDeparturesResponse = response
            .json()
            .await
            .context("Failed to parse stop_departures response")?;

        let mut departures = Vec::new();

        for route_dep in body.route_departures {
            for merged in route_dep.merged_itineraries {
                let headsign = merged
                    .itineraries
                    .first()
                    .and_then(|i| i.merged_headsign.clone().or_else(|| i.headsign.clone()));

                for item in merged.schedule_items {
                    let scheduled = item.scheduled_departure_time.unwrap_or(item.departure_time);
                    let delay_seconds = item
                        .is_real_time
                        .then(|| (item.departure_time - scheduled) as i32);

                    departures.push(Departure {
                        global_stop_id: route_dep.global_stop_id.clone(),
                        global_route_id: route_dep.global_route_id.clone(),
                        route_short_name: route_dep.route_short_name.clone(),
                        headsign: headsign.clone(),
                        departure_time: item.departure_time,
                        scheduled_departure_time: scheduled,
                        delay_seconds,
                        is_real_time: item.is_real_time,
                        is_cancelled: item.is_cancelled,
                        rt_trip_id: item.rt_trip_id,
                    });
                }
            }
        }

        departures.sort_by_key(|d| d.departure_time);
        Ok(departures)
    }

    /// One-time: log all route IDs near UCLA to find BBB global_route_ids.
    pub async fn discover_route_id(&self) -> Result<()> {
        let response = self
            .client
            .get(format!("{}/v4/public/nearby_routes", TRANSIT_API_BASE))
            .header("apiKey", &self.api_key)
            .query(&[
                ("lat", DISCOVERY_LAT.to_string()),
                ("lon", DISCOVERY_LON.to_string()),
                ("max_distance", "300".to_string()),
                ("should_update_realtime", "false".to_string()),
                ("max_num_departures", "0".to_string()),
            ])
            .send()
            .await
            .context("Failed to call nearby_routes")?;

        if !response.status().is_success() {
            anyhow::bail!("Transit API returned {}", response.status());
        }

        let body: DiscoverRoutesResponse = response
            .json()
            .await
            .context("Failed to parse nearby_routes response")?;

        tracing::info!("=== Route Discovery Results ===");
        for route in &body.nearby_routes {
            tracing::info!(
                global_route_id = %route.global_route_id,
                short_name = %route.route_short_name,
                network = %route.route_network_name.as_deref().unwrap_or("?"),
                "Found route"
            );
        }

        Ok(())
    }

    /// One-time: find stop IDs near a coordinate for populating ROUTE_IDS.
    pub async fn resolve_stops(&self, lat: f64, lon: f64) -> Result<Vec<NearbyStop>> {
        let response = self
            .client
            .get(format!("{}/v4/public/nearby_stops", TRANSIT_API_BASE))
            .header("apiKey", &self.api_key)
            .query(&[
                ("lat", lat.to_string()),
                ("lon", lon.to_string()),
                ("max_distance", "200".to_string()),
            ])
            .send()
            .await
            .context("Failed to call nearby_stops")?;

        if !response.status().is_success() {
            anyhow::bail!("nearby_stops returned {}", response.status());
        }

        let body: NearbyStopsResponse = response
            .json()
            .await
            .context("Failed to parse nearby_stops response")?;

        Ok(body.stops)
    }
}
