#!/usr/bin/env bash
# Migrates existing bus_tracking.db (SQLite) into the Postgres container.
# Usage: ./migrate.sh
# Requires: sqlite3, psql (postgresql-client)
set -euo pipefail

SQLITE_DB="./bus_tracking.db"
PG_URL="${DATABASE_URL:-postgres://bbb:bbb@localhost:5432/bbb}"

if [ ! -f "$SQLITE_DB" ]; then
  echo "No bus_tracking.db found — nothing to migrate."
  exit 0
fi

echo "==> Counting rows to migrate..."
STOP_COUNT=$(sqlite3 "$SQLITE_DB" "SELECT COUNT(*) FROM stops;")
DEP_COUNT=$(sqlite3 "$SQLITE_DB" "SELECT COUNT(*) FROM departure_log;")
echo "    stops: $STOP_COUNT  departure_log: $DEP_COUNT"

echo "==> Exporting stops..."
sqlite3 "$SQLITE_DB" <<'EOF'
.mode csv
.headers off
.output /tmp/bbb_stops.csv
SELECT global_stop_id, stop_name, lat, lon FROM stops;
EOF

echo "==> Exporting departure_log..."
sqlite3 "$SQLITE_DB" <<'EOF'
.mode csv
.headers off
.output /tmp/bbb_departure_log.csv
SELECT polled_at, global_stop_id, global_route_id, route_short_name, headsign,
       departure_time, scheduled_departure_time, delay_seconds,
       CASE is_real_time WHEN 1 THEN 'true' ELSE 'false' END,
       CASE is_cancelled WHEN 1 THEN 'true' ELSE 'false' END,
       rt_trip_id
FROM departure_log;
EOF

echo "==> Loading stops into Postgres..."
psql "$PG_URL" <<'SQL'
CREATE TEMP TABLE stops_import (LIKE stops);
\COPY stops_import (global_stop_id, stop_name, lat, lon) FROM '/tmp/bbb_stops.csv' CSV
INSERT INTO stops SELECT * FROM stops_import ON CONFLICT DO NOTHING;
SQL

echo "==> Loading departure_log into Postgres (this may take a moment)..."
psql "$PG_URL" -c "\COPY departure_log (polled_at, global_stop_id, global_route_id, route_short_name, headsign, departure_time, scheduled_departure_time, delay_seconds, is_real_time, is_cancelled, rt_trip_id) FROM '/tmp/bbb_departure_log.csv' CSV"

echo "==> Verifying..."
psql "$PG_URL" -c "SELECT COUNT(*) AS stops FROM stops;"
psql "$PG_URL" -c "SELECT COUNT(*) AS departures FROM departure_log;"

echo "Done."
