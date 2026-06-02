# Big Blue Bus Transit Tracker

Local transit data mirror for West LA bus advocacy. Polls the Transit App API for real-time
departure predictions across Big Blue Bus Route 1, Culver CityBus 6R, and key watch-point
stops for routes 6 and 17. Stores everything in SQLite and serves it through a local Axum
API so downstream apps (e-ink dashboard, analysis scripts) don't burn API quota.

## Motivation

Big Blue Bus Route 1 (Westwood ↔ UCLA) has severe reliability problems:

- **Bus bunching** — two or three buses arrive within one minute, followed by 20–40+ minute gaps
- **Terminus bunching** — buses leave UCLA already bunched, before traffic is even a factor
- **Ghost buses** — shows up in Transit app, never arrives
- **42-minute service gaps** have been personally documented

This tool collects continuous departure data to support advocacy at LA City Council, Santa Monica
City Council, and Metro Board for dedicated bus lanes, signal priority, and a Route 1 rapid
equivalent (similar to how 6R compares to local 6).

## Architecture

```
Transit App API  ──(bootstrap, once)──▶  route_details   ──▶  stops table
                 ──(every 15 min) ────▶  stop_departures  ──▶  departure_log table
                                                               │
                                                         Axum API server
                                                         GET /api/departures
                                                         GET /api/status
```

### Two binaries

| Binary | Command | Purpose |
|--------|---------|---------|
| `server` | `cargo run --bin server` | Polling loop + API server (runs continuously) |
| `bigbluebunch` | `cargo run --bin bigbluebunch` | CLI helpers (--discover, --resolve-stops) |

### Poll loop

- **Active window**: 7–10am and 4–7pm, **weekdays only**
- **Interval**: 15 minutes (900 s)
- **Rate limiting**: 13-second delay between every API call
- **Budget**: ~48 calls/day × 22 weekdays = ~1056/month (well under the 1500/month cap)
- Polls immediately on startup so the cache is never empty at launch

### Bootstrap (runs once, ever)

On first run the `stops` table is empty. The server fetches `route_details` for each route
in `ROUTE_IDS` (one API call per route, 13 s apart) to populate all stop coordinates. After
bootstrap the stops table is never written again — the data is static.

### Stop coverage

| Source | Routes | How |
|--------|--------|-----|
| `ROUTE_IDS` bootstrap | Route 1 (BBB:14412), 6R (CCBCA:77951) | Full route, all stops |
| `EXTRA_STOP_IDS` | Routes 6, 17, LADOT, others | Specific watch-point stops only |

Each `stop_departures` call accepts up to 100 stop IDs. With ~150 total stops the poll fits
in 2 API calls per cycle.

## Quick Start

```bash
# 1. Copy and fill in the env file
cp .env.example .env   # set TRANSIT_API_KEY, ROUTE_IDS, EXTRA_STOP_IDS

# 2. Start the server (Nix shell recommended)
nix develop --command cargo run --bin server

# Query the API
curl http://localhost:8080/api/status
curl "http://localhost:8080/api/departures?stop_ids=BBB:7023,MLA:107070"
```

### Environment variables

| Variable | Required | Description |
|----------|----------|-------------|
| `TRANSIT_API_KEY` | yes | Transit App public API key |
| `ROUTE_IDS` | yes | Comma-separated global route IDs to bootstrap (e.g. `BBB:14412,CCBCA:77951`) |
| `EXTRA_STOP_IDS` | no | Additional stop IDs to poll (watch-point stops for routes not bootstrapped) |
| `PORT` | no | API server port (default: 8080) |
| `RUST_LOG` | no | Log level (default: info) |

### CLI helpers

```bash
# Find route IDs near UCLA
cargo run --bin bigbluebunch -- --discover

# Find stop IDs near a GPS coordinate
cargo run --bin bigbluebunch -- --resolve-stops 34.0689 -118.4452
```

## API

### `GET /api/departures`

Returns the latest poll results from the in-memory cache. Returns 503 if no poll has completed yet.

**Query params**: `stop_ids` — comma-separated global stop IDs to filter by (optional; omit for all stops)

```bash
curl "http://localhost:8080/api/departures?stop_ids=BBB:7023,MLA:107070"
```

```json
{
  "polled_at": 1748123456,
  "departures": [
    {
      "global_stop_id": "BBB:7023",
      "global_route_id": "BBB:14412",
      "route_short_name": "1",
      "headsign": "UCLA",
      "departure_time": 1748123600,
      "scheduled_departure_time": 1748123580,
      "delay_seconds": 20,
      "is_real_time": true,
      "is_cancelled": false,
      "rt_trip_id": "2047030_1355_76320"
    }
  ]
}
```

### `GET /api/stops`

Returns all stops with coordinates. Loaded from DB at startup, served from memory.

```json
[
  { "global_stop_id": "BBB:7023", "stop_name": "Barrington & Santa Monica", "lat": 34.0321, "lon": -118.4812 }
]
```

### `GET /api/status`

```json
{
  "status": "ok",
  "last_polled_at": 1748123456,
  "departure_count": 2700,
  "stop_count": 126,
  "timestamp": 1748123500
}
```

## Database Schema

```sql
-- Static stop metadata, bootstrapped once
CREATE TABLE stops (
    global_stop_id TEXT PRIMARY KEY,
    stop_name TEXT NOT NULL,
    lat REAL NOT NULL,
    lon REAL NOT NULL
);

-- Append-only departure log, one row per departure per poll
CREATE TABLE departure_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    polled_at INTEGER NOT NULL,       -- Unix timestamp of poll
    global_stop_id TEXT NOT NULL,
    global_route_id TEXT NOT NULL,
    route_short_name TEXT NOT NULL,
    headsign TEXT,
    departure_time INTEGER NOT NULL,
    scheduled_departure_time INTEGER NOT NULL,
    delay_seconds INTEGER,
    is_real_time INTEGER NOT NULL DEFAULT 0,
    is_cancelled INTEGER NOT NULL DEFAULT 0,
    rt_trip_id TEXT
);

CREATE INDEX idx_log_polled_at ON departure_log(polled_at);
CREATE INDEX idx_log_stop ON departure_log(global_stop_id, departure_time);
```

### Useful queries

```bash
sqlite3 bus_tracking.db
```

```sql
-- How many departures logged total?
SELECT COUNT(*) FROM departure_log;

-- Departures per poll session (confirms polling is running)
SELECT datetime(polled_at, 'unixepoch', 'localtime') as time, COUNT(*) as deps
FROM departure_log GROUP BY polled_at ORDER BY polled_at DESC LIMIT 20;

-- Routes captured and their departure counts
SELECT route_short_name, COUNT(*) as dep_count
FROM departure_log
GROUP BY route_short_name
ORDER BY dep_count DESC;

-- Find stop IDs for a street name
SELECT global_stop_id, stop_name FROM stops WHERE stop_name LIKE '%Barrington%';
```

#### Headway analysis (scheduled vs actual gaps)

Deduplicates across poll sessions so each real departure is counted once.
Replace `global_stop_id` and `route_short_name` with your stop of interest.

```sql
WITH deduped AS (
    SELECT DISTINCT scheduled_departure_time, departure_time
    FROM departure_log
    WHERE global_stop_id = 'BBB:6863'
      AND route_short_name = '1'
      AND is_real_time = 1
),
t AS (
    SELECT
        scheduled_departure_time,
        departure_time,
        LAG(scheduled_departure_time) OVER (ORDER BY scheduled_departure_time) as prev_sched,
        LAG(departure_time)           OVER (ORDER BY scheduled_departure_time) as prev_actual
    FROM deduped
)
SELECT
    datetime(scheduled_departure_time, 'unixepoch', 'localtime') as sched_at,
    ROUND((scheduled_departure_time - prev_sched) / 60.0, 0)     as sched_gap_min,
    ROUND((departure_time - prev_actual) / 60.0, 0)              as actual_gap_min,
    ROUND((departure_time - scheduled_departure_time) / 60.0, 1) as delay_min
FROM t
WHERE prev_sched IS NOT NULL
  AND (scheduled_departure_time - prev_sched) > 60  -- filter out sub-minute dupes
ORDER BY scheduled_departure_time;
```

Bunching shows as `actual_gap_min` much smaller than `sched_gap_min` on consecutive rows
(one bus early, the next close behind). A large `actual_gap_min` is a service gap.

#### Worst service gaps at a stop

```sql
WITH deduped AS (
    SELECT DISTINCT scheduled_departure_time, departure_time
    FROM departure_log
    WHERE global_stop_id = 'BBB:6863' AND route_short_name = '1'
),
t AS (
    SELECT departure_time,
           LAG(departure_time) OVER (ORDER BY departure_time) as prev
    FROM deduped
)
SELECT
    datetime(departure_time, 'unixepoch', 'localtime') as at,
    ROUND((departure_time - prev) / 60.0, 1)           as gap_min
FROM t
WHERE prev IS NOT NULL
ORDER BY gap_min DESC LIMIT 20;
```

#### Average delay by hour of day (advocacy: peak hours worst?)

```sql
SELECT
    strftime('%H', scheduled_departure_time, 'unixepoch', 'localtime') as hour,
    ROUND(AVG(departure_time - scheduled_departure_time) / 60.0, 1)    as avg_delay_min,
    COUNT(*) as samples
FROM departure_log
WHERE route_short_name = '1' AND is_real_time = 1
GROUP BY hour
ORDER BY hour;
```

## Project Structure

```
bigbluebunch/
├── Cargo.toml
├── flake.nix                  # Nix dev shell (rustc, sqlx-cli, jq, protobuf)
├── .env                       # API key + route/stop config (not committed)
├── bus_tracking.db            # SQLite database (created at runtime)
├── static/
│   └── map.html               # Leaflet departure map (embedded at compile time)
├── src/
│   ├── lib.rs                 # Module exports
│   ├── main.rs                # CLI binary (--discover, --resolve-stops)
│   ├── api.rs                 # Transit App API client
│   ├── api_server.rs          # Axum server (/, /api/departures, /api/stops, /api/status)
│   ├── db.rs                  # SQLite layer (stops + departure_log)
│   ├── models.rs              # Stop, Departure, PollResult structs
│   └── bin/
│       └── server.rs          # Server binary (poll loop + HTTP server)
└── proto/
    └── gtfs-realtime.proto    # Kept for reference (not currently used)
```

## Transit App API

Base URL: `https://external.transitapp.com/v4/public`

| Endpoint | Calls/month | When |
|----------|-------------|------|
| `route_details?global_route_id=X` | 2 (once, on bootstrap) | First run only |
| `stop_departures?global_stop_ids=...` | ~1056 (2/poll × 24 polls/day × 22 days) | Every active window |

Rate limit: 5 calls/minute. The server enforces 13 s between every call.
