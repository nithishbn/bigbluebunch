# Big Blue Bus Route 1 Bunching Tracker - Technical Documentation

## Project Purpose

This application monitors Big Blue Bus Route 1 to collect evidence of service reliability issues for transit advocacy. The goal is to gather comprehensive data over 3+ weeks documenting bus bunching, service gaps, and operational problems to support advocacy for better bus service, dedicated bus lanes, and signal priority.

## Problem Statement

Big Blue Bus Route 1 (Westwood to UCLA) experiences severe reliability issues:
- **Bus bunching**: Multiple buses (2-3) arriving within 1 minute, followed by 20-40+ minute gaps
- **Terminus bunching**: Buses departing UCLA already bunched before encountering traffic
- **Ghost buses**: Scheduled buses appearing in apps but never arriving
- **Extreme service gaps**: Up to 42-minute gaps after bunched buses
- **Driver variability**: Same route taking 15-25 minutes depending on driver behavior

This data will support advocacy at LA City Council, Santa Monica City Council, and Metro Board meetings for infrastructure and operational improvements.

## Technical Implementation

### Core Architecture

The application follows a simple poll-process-store loop:

```
┌─────────────────────────────────────────────────────────┐
│                    Main Loop (Tokio)                     │
│                  60-second interval                      │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                   GTFS-RT Client                         │
│  HTTP GET: gtfs.bigbluebus.com/vehiclepositions.bin     │
│                   (Binary Protobuf)                      │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                  Protobuf Parser                         │
│     Decode FeedMessage → Extract Entities → Filter      │
│              Route ID == "1" (Route 1)                   │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                  BusObservation Structs                  │
│   {timestamp, vehicle_id, lat, lon, trip_id, ...}       │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│              SQLite Database (sqlx)                      │
│         Async transaction batch insert                   │
│            observations table                            │
└─────────────────────────────────────────────────────────┘
```

### Module Breakdown

#### `src/main.rs` - Application Entry Point

**Responsibilities**:
- Initialize async runtime with Tokio
- Set up database connection
- Configure logging
- Run infinite polling loop with 60-second interval
- Display real-time statistics
- Handle errors without crashing

**Key Functions**:
- `main()`: Async entry point
  - Creates `Database` instance
  - Creates `GtfsClient` instance
  - Runs `tokio::time::interval` loop
  - Calls `client.poll_route_1()` every 60 seconds
  - Logs results and saves to database

**Error Handling Strategy**:
- Catch all errors in poll loop
- Log errors with context
- Continue to next iteration
- Never panic or exit on transient failures

---

#### `src/api.rs` - GTFS-RT API Client

**Responsibilities**:
- HTTP communication with Big Blue Bus API
- Protobuf decoding
- Data transformation from GTFS-RT types to application types
- Filtering for Route 1

**Key Types**:
- `GtfsClient`: HTTP client wrapper
  - `client: reqwest::Client` with 10-second timeout

**Key Methods**:

```rust
pub async fn fetch_vehicle_positions(&self) -> Result<Bytes>
```
- HTTP GET to `http://gtfs.bigbluebus.com/vehiclepositions.bin`
- Returns raw binary protobuf data
- Validates HTTP status code

```rust
pub fn parse_feed(&self, data: &[u8]) -> Result<Vec<BusObservation>>
```
- Decodes protobuf using `prost::Message::decode()`
- Iterates through `feed.entity` array
- Extracts vehicle position data
- Maps to `BusObservation` structs
- Returns all vehicles (not filtered)

```rust
pub async fn poll_route_1(&self) -> Result<(Vec<BusObservation>, PollStats)>
```
- High-level method combining fetch + parse + filter
- Calls `fetch_vehicle_positions()`
- Calls `parse_feed()`
- Filters observations where `route_id == "1"`
- Returns Route 1 observations + statistics

**Protobuf Integration**:
```rust
pub mod gtfs_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}
```
- Includes generated code from `build.rs`
- Provides `FeedMessage`, `FeedEntity`, `VehiclePosition` types
- Generated at compile time from `proto/gtfs-realtime.proto`

---

#### `src/models.rs` - Data Structures

**`BusObservation`**:
Core data structure representing a single bus position at a point in time.

```rust
pub struct BusObservation {
    pub timestamp: i64,              // Unix timestamp
    pub vehicle_id: String,          // Bus number (e.g., "12345")
    pub route_id: String,            // Route identifier (e.g., "1")
    pub trip_id: Option<String>,     // GTFS trip ID
    pub direction_id: Option<i32>,   // 0 or 1 (inbound/outbound)
    pub latitude: f64,               // GPS latitude
    pub longitude: f64,              // GPS longitude
    pub current_stop_sequence: Option<i32>,  // Stop number
    pub speed: Option<f32>,          // m/s
    pub bearing: Option<f32>,        // Degrees (0-360)
}
```

**Design Decisions**:
- `Option<T>` for fields that may be missing in GTFS-RT feed
- `i64` for timestamp (Unix epoch seconds)
- `f64` for coordinates (standard GPS precision)
- `f32` for speed/bearing (adequate precision, smaller storage)
- Implements `Display` for human-readable logging

**Methods**:
- `new()`: Constructor with required fields only
- `is_route_1()`: Convenience method for filtering

**`PollStats`**:
Statistics about a single polling cycle.

```rust
pub struct PollStats {
    pub total_vehicles: usize,      // All vehicles in feed
    pub route_1_vehicles: usize,    // Route 1 buses only
    pub timestamp: i64,             // When poll occurred
}
```

---

#### `src/db.rs` - Database Layer

**Responsibilities**:
- SQLite connection management
- Schema initialization
- Async insert operations
- Query methods for statistics

**Key Type**:
```rust
pub struct Database {
    pool: SqlitePool,  // Connection pool (max 5)
}
```

**Key Methods**:

```rust
pub async fn new(path: &str) -> Result<Self>
```
- Creates SQLite database file if doesn't exist
- Opens connection pool
- Calls `init_schema()` to create tables/indexes
- Returns `Database` instance

```rust
async fn init_schema(&self) -> Result<()>
```
- Creates `observations` table if not exists
- Creates indexes on `timestamp` and `vehicle_id`
- Idempotent (safe to call multiple times)

```rust
pub async fn insert_observations(&self, observations: &[BusObservation]) -> Result<usize>
```
- **Batch insert** using transactions for efficiency
- Begins transaction with `pool.begin()`
- Inserts each observation
- Commits transaction
- Returns count of inserted rows
- Rolls back on error (transaction safety)

```rust
pub async fn count_observations(&self) -> Result<i64>
pub async fn count_route_1_observations(&self) -> Result<i64>
```
- Query methods for statistics
- Used for logging current database state

**SQL Schema**:
```sql
CREATE TABLE observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    vehicle_id TEXT NOT NULL,
    route_id TEXT NOT NULL,
    trip_id TEXT,
    direction_id INTEGER,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    current_stop_sequence INTEGER,
    speed REAL,
    bearing REAL
);

CREATE INDEX idx_timestamp ON observations(timestamp);
CREATE INDEX idx_vehicle ON observations(vehicle_id);
```

**Why SQLite**:
- Single-file database (easy to backup/transfer)
- No server setup required
- Excellent for time-series append operations
- Sufficient for millions of rows
- Easy to query with standard SQL

**Why sqlx**:
- Compile-time SQL validation (when using macros)
- Async/await support (works with Tokio)
- Connection pooling
- Transaction support
- Type-safe query results

---

### Protobuf Build Process

**`build.rs`**:
```rust
fn main() {
    prost_build::compile_protos(&["proto/gtfs-realtime.proto"], &["proto/"])
        .expect("Failed to compile protobuf definitions");
}
```

**What Happens**:
1. Cargo runs `build.rs` before compiling `src/`
2. `prost-build` reads `proto/gtfs-realtime.proto`
3. Generates Rust structs/enums for all protobuf messages
4. Writes to `$OUT_DIR/transit_realtime.rs`
5. `src/api.rs` includes this file with `include!()` macro

**Generated Types**:
- `FeedMessage`: Top-level container
- `FeedHeader`: Metadata about feed
- `FeedEntity`: Individual vehicle/alert/trip update
- `VehiclePosition`: Bus location data
- `TripDescriptor`: Trip information
- `VehicleDescriptor`: Vehicle identification
- `Position`: Lat/lon/bearing/speed

**Why Protobuf**:
- Efficient binary format (smaller than JSON)
- Strongly typed (compile-time validation)
- Standard for GTFS-RT (all transit agencies use this)
- Fast parsing (no string parsing overhead)

---

## GTFS-RT API Specification

### Corrected Endpoint Information

**Original Brief Had**:
- URL: `https://gtfs.bigbluebus.com/gtfsrt/vehiclepositions.pb` (WRONG - 404)

**Actual Working Endpoint**:
- URL: `http://gtfs.bigbluebus.com/vehiclepositions.bin` (CORRECT)

**Other Endpoints**:
- Trip Updates: `http://gtfs.bigbluebus.com/tripupdates.bin`
- Alerts: `http://gtfs.bigbluebus.com/alerts.bin`

**Protocol**:
- HTTP (not HTTPS)
- GET request
- No authentication required
- No API key needed
- Public endpoint
- CORS enabled

**Response**:
- Content-Type: `application/octet-stream`
- Format: GTFS-realtime Protocol Buffer (binary)
- Size: 15 bytes (empty) to several KB (with active buses)
- Update frequency: ~30-60 seconds

### GTFS-RT Feed Structure

The Protocol Buffer message hierarchy:

```
FeedMessage {
  header: FeedHeader {
    gtfs_realtime_version: "1.0"
    timestamp: 1738224431  // Unix epoch
  }
  entity: [
    FeedEntity {
      id: "12345"
      vehicle: VehiclePosition {
        trip: TripDescriptor {
          trip_id: "2047030_1355_76320"
          route_id: "1"          // ← Filter on this
          direction_id: 0        // 0=outbound, 1=inbound
        }
        vehicle: VehicleDescriptor {
          id: "12345"            // Bus number
        }
        position: Position {
          latitude: 34.0689
          longitude: -118.4452
          bearing: 180.0         // Degrees
          speed: 5.5             // m/s
        }
        timestamp: 1738224431
        current_stop_sequence: 15
      }
    },
    // ... more entities
  ]
}
```

### Parsing Logic

**Filter Condition**:
```rust
if let Some(trip) = vehicle.trip {
    if let Some(route_id) = trip.route_id {
        if route_id == "1" {
            // This is a Route 1 bus
        }
    }
}
```

**Field Availability**:
- `trip` and `vehicle` are optional in protobuf spec
- `position` required for our use case (skip if missing)
- `trip_id`, `direction_id`, `current_stop_sequence` often present but optional
- `speed` and `bearing` sometimes missing

**Timestamp Priority**:
1. Use `vehicle.timestamp` if available (most accurate)
2. Fall back to `header.timestamp` (feed generation time)
3. Never missing (both are required in GTFS-RT spec)

---

## Data Collection Strategy

### MVP Scope (Current Implementation)

**What It Does**:
- Polls every 60 seconds
- Filters for Route 1
- Logs vehicle positions
- Stores in SQLite
- Handles errors gracefully

**What It Doesn't Do** (Deferred to Analysis Phase):
- Real-time bunching detection
- Bunching alerts/notifications
- Distance calculation along route
- Web dashboard
- Data visualization

**Philosophy**:
Collect raw data first, analyze later. Bunching detection can be done offline with SQL queries after data collection is complete.

### Expected Data Volume

**3 Weeks of Collection**:
- Polls: ~30,240 (every 60 seconds)
- Route 1 observations: 3,000-5,000+ (assuming 10-15% of polls have active buses)
- Database size: ~500KB-2MB
- Unique vehicles: 10-20 buses

**Storage Calculation**:
- Per observation: ~100 bytes (row + indexes)
- 5,000 observations × 100 bytes = 500KB
- Plus SQLite overhead: ~1-2MB total

---

## Error Handling Philosophy

### Transient vs. Fatal Errors

**Transient** (retry on next poll):
- Network timeouts
- HTTP 5xx errors
- Connection refused
- Malformed protobuf (skip this poll)
- Database locked (rare with SQLite)

**Fatal** (should crash):
- Cannot create database file (disk full, permissions)
- Invalid database path
- Build-time errors (missing proto file)

### Implementation

All polling loop errors are caught and logged:
```rust
match client.poll_route_1().await {
    Ok((observations, stats)) => { /* process */ }
    Err(e) => {
        log::error!("Poll failed: {}", e);
        log::warn!("Will retry on next interval...");
        // Continue loop, don't crash
    }
}
```

Database errors are also non-fatal:
```rust
match db.insert_observations(&observations).await {
    Ok(count) => { /* success */ }
    Err(e) => {
        log::error!("Failed to save observations: {}", e);
        // Data lost for this poll, but continue
    }
}
```

### Logging Strategy

**Log Levels**:
- `ERROR`: Failed operations (network, parse, database)
- `WARN`: Recoverable issues (will retry)
- `INFO`: Normal operation (poll results, statistics)
- `DEBUG`: Verbose details (API responses, byte counts)

**Configurable via Environment**:
```bash
RUST_LOG=info cargo run       # Default
RUST_LOG=debug cargo run      # Verbose
RUST_LOG=bigbluebunch::api=debug cargo run  # Module-specific
```

---

## Analysis Approach (Post-Collection)

### SQL Queries for Bunching Detection

After 3 weeks of data collection, analyze bunching with SQL:

**Gap Analysis** (time between consecutive buses):
```sql
WITH bus_times AS (
  SELECT
    timestamp,
    vehicle_id,
    LAG(timestamp) OVER (ORDER BY timestamp) as prev_timestamp
  FROM observations
  WHERE route_id = '1'
)
SELECT
  vehicle_id,
  datetime(timestamp, 'unixepoch') as time,
  (timestamp - prev_timestamp) / 60.0 as gap_minutes
FROM bus_times
WHERE gap_minutes < 3  -- Bunched (less than 3 min)
   OR gap_minutes > 15; -- Service gap (more than 15 min)
```

**Distance-Based Bunching** (buses within 500m):
```sql
-- Requires implementing Haversine distance function
-- Or export to Python/pandas for geospatial analysis
```

**Bunching Frequency by Hour**:
```sql
SELECT
  strftime('%H', timestamp, 'unixepoch') as hour,
  COUNT(*) as observations,
  COUNT(DISTINCT vehicle_id) as unique_buses
FROM observations
WHERE route_id = '1'
GROUP BY hour
ORDER BY hour;
```

### Export for Advanced Analysis

```bash
sqlite3 bus_tracking.db <<EOF
.mode csv
.output observations.csv
SELECT * FROM observations WHERE route_id = '1';
EOF
```

Then use Python/pandas for:
- Geospatial analysis (geopandas)
- Time-series visualization (matplotlib)
- Statistical analysis (scipy)
- Interactive maps (folium)

---

## Future Enhancements (Out of Scope for MVP)

### Real-Time Bunching Detection

Add `src/bunching.rs` module:
- Calculate distance between consecutive buses
- Track headways (time between buses)
- Insert into `bunching_events` table
- Log alerts when bunching detected

### Haversine Distance Calculation

For distance-based bunching:
```rust
pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371000.0; // Earth radius in meters
    // ... formula implementation
}
```

### Web Dashboard

Real-time visualization:
- Axum web server
- WebSocket for live updates
- Leaflet.js map showing bus positions
- Chart.js for statistics
- Historical playback

### Scheduled Analysis

Cron job to run daily queries:
- Generate summary statistics
- Export to CSV
- Email reports
- Detect anomalies

---

## Deployment Considerations

### Database Backup

```bash
# Copy SQLite file
cp bus_tracking.db bus_tracking_backup_$(date +%Y%m%d).db

# Or use SQLite backup API
sqlite3 bus_tracking.db ".backup bus_tracking_backup.db"
```

### Monitoring

Check if process is running:
```bash
ps aux | grep bigbluebunch
```

Check recent logs (if using systemd):
```bash
journalctl -u bigbluebunch -f
```

Check database growth:
```bash
ls -lh bus_tracking.db
sqlite3 bus_tracking.db "SELECT COUNT(*) FROM observations;"
```

### Resource Usage

**Expected**:
- CPU: <1% (mostly idle, wakes every 60s)
- Memory: ~10-20MB (Tokio runtime + connection pool)
- Disk I/O: Minimal (batch inserts every 60s)
- Network: ~1-5KB per poll
- Disk space: ~1-2MB for 3 weeks

**Very lightweight** - can run on minimal hardware (Raspberry Pi, cheap VPS).

---

## Testing Strategy

### Unit Tests

Test protobuf parsing with fixture data:
```rust
#[test]
fn test_parse_empty_feed() {
    let client = GtfsClient::new();
    let data = include_bytes!("fixtures/empty_feed.bin");
    let result = client.parse_feed(data).unwrap();
    assert_eq!(result.len(), 0);
}
```

### Integration Tests

Test full poll cycle with real API:
```rust
#[tokio::test]
async fn test_poll_route_1() {
    let client = GtfsClient::new();
    let result = client.poll_route_1().await;
    assert!(result.is_ok());
}
```

### Database Tests

Test insert and query operations:
```rust
#[tokio::test]
async fn test_insert_observation() {
    let db = Database::new(":memory:").await.unwrap();
    let obs = BusObservation::new(/*...*/);
    db.insert_observation(&obs).await.unwrap();
    assert_eq!(db.count_observations().await.unwrap(), 1);
}
```

---

## Dependencies Justification

### Why These Specific Crates?

**Tokio**: De facto standard async runtime, excellent documentation, production-ready

**reqwest**: Most popular HTTP client, integrates with Tokio, supports timeouts

**prost**: Preferred protobuf library for Rust, better than rust-protobuf (unmaintained)

**sqlx**: Async database library, compile-time SQL checking, connection pooling

**chrono**: Standard datetime library, ergonomic timestamp handling

**anyhow**: Simple error handling with context chains, perfect for applications

**log + env_logger**: Flexible logging facade, environment-based configuration

### Alternative Considered

- **rusqlite** instead of sqlx: Synchronous, would block Tokio runtime
- **quick-protobuf** instead of prost: Less maintained, smaller ecosystem
- **actix-web runtime** instead of tokio: Overkill for simple polling

---

## Summary

This is a **focused, MVP-scoped data collection tool** designed to:
1. Run reliably for weeks without intervention
2. Collect comprehensive bus position data
3. Handle failures gracefully
4. Enable offline analysis

The architecture prioritizes **simplicity** and **reliability** over real-time features. Analysis happens after data collection is complete, using SQL and Python tools.

**Key Design Decisions**:
- Async Rust with Tokio for efficient I/O
- SQLite for zero-configuration storage
- Protobuf for standard GTFS-RT compatibility
- Append-only database for reliability
- Graceful error handling for 24/7 operation
- Deferred analysis for simpler implementation

This approach minimizes complexity while maximizing data quality and system reliability.
