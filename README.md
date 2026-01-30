# Big Blue Bus Route 1 Bunching Tracker

Automated monitoring system for Big Blue Bus Route 1 that polls GTFS real-time data every 60 seconds to document bus bunching incidents and service reliability issues.

## Motivation

I'm a UCLA PhD student who regularly rides Big Blue Bus Route 1 from Westwood to campus. The service is extremely unreliable:

- **Bus bunching**: Multiple buses arrive together (sometimes 2-3 buses within 1 minute), then 20-40+ minute gaps
- **Terminus bunching**: Buses leave UCLA (the route terminus) bunched together, even before traffic is a factor
- **Ghost buses**: Buses that show up in Transit app but never arrive
- **Driver variability**: Same trip takes 15-25 minutes depending on how aggressively the driver merges into traffic
- **Service gaps**: I've experienced 42-minute gaps after bunched buses, forcing me to take expensive Uber rides

**Goal**: Document these failures with comprehensive data to advocate for:
1. Better Big Blue Bus operational management (dispatching, recovery time)
2. Dedicated bus lanes on Santa Monica Blvd
3. Signal priority for buses

## Quick Start

```bash
# Build the project
cargo build --release

# Run the tracker
cargo run --release

# Run with debug logging
RUST_LOG=debug cargo run --release
```

The tracker will:
- Poll the GTFS-RT API every 60 seconds
- Filter for Route 1 buses
- Display current bus positions in logs
- Save observations to SQLite database (`bus_tracking.db`)

## Architecture

### Data Flow

```
GTFS-RT API → HTTP Fetch → Protobuf Parse → Filter Route 1 → SQLite Storage
     ↑                                                              ↓
     └──────────────── 60 second interval ─────────────────────────┘
```

### Components

#### 1. API Client (`src/api.rs`)
- Fetches vehicle positions from: `http://gtfs.bigbluebus.com/vehiclepositions.bin`
- Decodes Protocol Buffer format using `prost`
- Parses GTFS-RT feed into structured data
- Filters for Route 1 buses
- Returns `Vec<BusObservation>` for each poll

#### 2. Data Models (`src/models.rs`)

**`BusObservation`**: Represents a single bus position with metadata
- `timestamp`: Unix timestamp when observation was recorded
- `vehicle_id`: Unique vehicle identifier (bus number)
- `route_id`: Route identifier (e.g., "1" for Route 1)
- `latitude`, `longitude`: GPS coordinates
- `trip_id`: GTFS trip identifier
- `direction_id`: Direction of travel (0 or 1)
- `current_stop_sequence`: Which stop bus is at/approaching
- `speed`: Speed in meters per second
- `bearing`: Heading in degrees

**`PollStats`**: Statistics about each polling cycle
- `total_vehicles`: All vehicles in feed
- `route_1_vehicles`: Count of Route 1 buses
- `timestamp`: When poll occurred

#### 3. Database (`src/db.rs`)
- SQLite storage using `sqlx` with async transactions
- Connection pooling (max 5 connections)
- Batch inserts for efficiency
- Query methods for statistics

#### 4. Main Loop (`src/main.rs`)
- Tokio async runtime for concurrency
- 60-second polling interval using `tokio::time::interval`
- Graceful error handling with retry logic
- Structured logging with `log` and `env_logger`

### Protobuf Setup

GTFS-realtime uses Protocol Buffers for efficient binary serialization:

1. **`proto/gtfs-realtime.proto`**: Official GTFS-RT schema from Google Transit
2. **`build.rs`**: Compile-time code generation using `prost-build`
3. **Generated code**: Rust structs in `$OUT_DIR/transit_realtime.rs`
4. **Usage**: Import generated types in `src/api.rs` as `gtfs_realtime` module

The build script runs before compilation and generates strongly-typed Rust structs from the protobuf definition.

## Database Schema

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

### Design Decisions

- **Unix timestamps**: Integer storage, easy to query by date/time ranges
- **Nullable fields**: Some GTFS-RT fields are optional (trip_id, direction_id, etc.)
- **Indexes**: On `timestamp` for time-series queries, `vehicle_id` for tracking specific buses
- **No foreign keys**: Simple append-only schema for reliability

## Dependencies

### Runtime Dependencies
- **tokio** (v1): Async runtime with full features
- **reqwest** (v0.11): HTTP client for API requests
- **prost** (v0.12): Protocol Buffer parser
- **prost-types** (v0.12): Well-known protobuf types
- **bytes** (v1): Efficient byte buffer handling
- **sqlx** (v0.8): Async SQLite with connection pooling
- **chrono** (v0.4): Timestamp parsing and formatting
- **anyhow** (v1): Ergonomic error handling
- **log** (v0.4): Logging facade
- **env_logger** (v0.11): Environment-based log configuration
- **serde** (v1): Serialization framework
- **serde_json** (v1): JSON support

### Build Dependencies
- **prost-build** (v0.12): Protobuf code generation

## Usage

### Running the Tracker

```bash
# Standard run with info-level logging
cargo run --release

# Debug mode for verbose logging
RUST_LOG=debug cargo run --release

# Specific module logging
RUST_LOG=bigbluebunch::api=debug cargo run --release
```

### Querying the Database

```bash
sqlite3 bus_tracking.db
```

#### Useful Queries

```sql
-- Total observations
SELECT COUNT(*) FROM observations;

-- Observations per day
SELECT DATE(timestamp, 'unixepoch') as date, COUNT(*)
FROM observations
GROUP BY date;

-- Unique vehicles tracked
SELECT COUNT(DISTINCT vehicle_id) FROM observations;

-- Recent Route 1 observations
SELECT
    datetime(timestamp, 'unixepoch') as time,
    vehicle_id,
    latitude,
    longitude,
    speed
FROM observations
WHERE route_id = '1'
ORDER BY timestamp DESC
LIMIT 10;

-- Observations by hour of day
SELECT
    strftime('%H', timestamp, 'unixepoch') as hour,
    COUNT(*) as count
FROM observations
WHERE route_id = '1'
GROUP BY hour
ORDER BY hour;
```

### Exporting Data

```sql
.mode csv
.output observations.csv
SELECT * FROM observations;
.quit
```

Export to CSV for analysis with Python (pandas), R, or Excel.

## GTFS-RT API Details

### Endpoint
- **URL**: `http://gtfs.bigbluebus.com/vehiclepositions.bin`
- **Method**: GET
- **Format**: Protocol Buffer (binary)
- **Authentication**: None required (public endpoint)
- **CORS**: Enabled (`Access-Control-Allow-Origin: *`)
- **Update Frequency**: Approximately 30-60 seconds
- **Content-Type**: `application/octet-stream`

### Additional Endpoints
- **Trip Updates**: `http://gtfs.bigbluebus.com/tripupdates.bin`
- **Alerts**: `http://gtfs.bigbluebus.com/alerts.bin`

### Response Structure

The GTFS-RT feed returns a `FeedMessage` containing vehicle position entities:

```
FeedMessage
├── header
│   └── timestamp (feed generation time)
└── entity[] (array of vehicles)
    └── vehicle
        ├── trip
        │   ├── trip_id: String
        │   ├── route_id: String (filter for "1")
        │   └── direction_id: Integer (0 or 1)
        ├── vehicle
        │   └── id: String (bus number)
        ├── position
        │   ├── latitude: Float
        │   ├── longitude: Float
        │   ├── bearing: Float (degrees)
        │   └── speed: Float (m/s)
        ├── timestamp: Integer (Unix time)
        └── current_stop_sequence: Integer
```

### Parsing Flow

1. **Fetch**: HTTP GET request to endpoint
2. **Decode**: Parse binary protobuf using `prost::Message::decode()`
3. **Extract**: Iterate through `feed.entity` array
4. **Filter**: Check `route_id == "1"`
5. **Transform**: Map protobuf types to `BusObservation` structs
6. **Store**: Batch insert into SQLite

## Error Handling

The application uses graceful degradation to handle failures without crashing:

### Network Errors
- **Timeout**: Log error, wait for next interval
- **Connection refused**: Log error, retry on next poll
- **HTTP error codes**: Log status code, continue polling

### Parse Errors
- **Malformed protobuf**: Log error with context, skip this poll
- **Missing required fields**: Skip individual entity, continue processing others

### Database Errors
- **Connection failure**: Log error, attempt reconnection
- **Insert failure**: Log error, continue to next poll
- **Transaction rollback**: Data consistency maintained

### Design Philosophy
**Never crash on transient failures.** The application is designed to run continuously for weeks, handling temporary API outages, network issues, and malformed data gracefully.

## Project Structure

```
bigbluebunch/
├── Cargo.toml              # Dependencies and project metadata
├── build.rs                # Protobuf code generation script
├── proto/
│   └── gtfs-realtime.proto # GTFS-RT protocol buffer definition
├── src/
│   ├── main.rs            # Entry point and polling loop
│   ├── api.rs             # GTFS-RT API client
│   ├── db.rs              # SQLite database layer
│   └── models.rs          # Data structures and types
├── bus_tracking.db        # SQLite database (created at runtime)
└── README.md              # This file
```

## Expected Data Collection

Running continuously for 3 weeks:
- **Polls**: ~30,240 (60s interval × 60 min × 24 hr × 21 days)
- **Observations**: 3,000-5,000+ Route 1 observations (varies by service hours)
- **Database size**: ~500KB-2MB (depending on observation count)
- **Unique vehicles**: 10-20 different buses
