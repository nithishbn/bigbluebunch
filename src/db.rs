use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, Row, PgPool};

use crate::models::{Departure, Stop};

pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool };
        db.init_schema().await?;
        Ok(db)
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS stops (
                global_stop_id TEXT PRIMARY KEY,
                stop_name TEXT NOT NULL,
                lat DOUBLE PRECISION NOT NULL,
                lon DOUBLE PRECISION NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create stops table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS departure_log (
                id BIGSERIAL PRIMARY KEY,
                polled_at BIGINT NOT NULL,
                global_stop_id TEXT NOT NULL,
                global_route_id TEXT NOT NULL,
                route_short_name TEXT NOT NULL,
                headsign TEXT,
                departure_time BIGINT NOT NULL,
                scheduled_departure_time BIGINT NOT NULL,
                delay_seconds INTEGER,
                is_real_time BOOLEAN NOT NULL DEFAULT FALSE,
                is_cancelled BOOLEAN NOT NULL DEFAULT FALSE,
                rt_trip_id TEXT
            )",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create departure_log table")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_log_polled_at ON departure_log(polled_at)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create polled_at index")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_log_stop ON departure_log(global_stop_id, departure_time)",
        )
        .execute(&self.pool)
        .await
        .context("Failed to create stop index")?;

        tracing::debug!("Database schema initialized");
        Ok(())
    }

    pub async fn stops_initialized(&self) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM stops")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count stops")?;
        Ok(count > 0)
    }

    pub async fn upsert_stops(&self, stops: &[Stop]) -> Result<()> {
        let mut tx = self.pool.begin().await.context("Failed to start transaction")?;

        for stop in stops {
            sqlx::query(
                "INSERT INTO stops (global_stop_id, stop_name, lat, lon)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT(global_stop_id) DO UPDATE SET
                   stop_name = EXCLUDED.stop_name,
                   lat = EXCLUDED.lat,
                   lon = EXCLUDED.lon",
            )
            .bind(&stop.global_stop_id)
            .bind(&stop.stop_name)
            .bind(stop.lat)
            .bind(stop.lon)
            .execute(&mut *tx)
            .await
            .context("Failed to upsert stop")?;
        }

        tx.commit().await.context("Failed to commit stops")?;
        Ok(())
    }

    pub async fn get_all_stop_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT global_stop_id FROM stops")
            .fetch_all(&self.pool)
            .await
            .context("Failed to query stop IDs")?;

        Ok(rows.iter().map(|r| r.get("global_stop_id")).collect())
    }

    pub async fn get_all_stops(&self) -> Result<Vec<Stop>> {
        let rows = sqlx::query("SELECT global_stop_id, stop_name, lat, lon FROM stops")
            .fetch_all(&self.pool)
            .await
            .context("Failed to query stops")?;

        Ok(rows
            .iter()
            .map(|r| Stop {
                global_stop_id: r.get("global_stop_id"),
                stop_name: r.get("stop_name"),
                lat: r.get("lat"),
                lon: r.get("lon"),
            })
            .collect())
    }

    pub async fn count_polls(&self) -> Result<(i64, i64)> {
        let row = sqlx::query(
            "SELECT
               COUNT(DISTINCT polled_at) AS total,
               COUNT(DISTINCT polled_at) FILTER (
                 WHERE polled_at >= EXTRACT(EPOCH FROM date_trunc('day', now()))::bigint
               ) AS today
             FROM departure_log",
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to count polls")?;
        Ok((row.get("total"), row.get("today")))
    }

    pub async fn insert_departure_log(
        &self,
        polled_at: i64,
        departures: &[Departure],
    ) -> Result<usize> {
        if departures.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await.context("Failed to start transaction")?;

        for dep in departures {
            sqlx::query(
                "INSERT INTO departure_log (
                    polled_at, global_stop_id, global_route_id, route_short_name, headsign,
                    departure_time, scheduled_departure_time, delay_seconds,
                    is_real_time, is_cancelled, rt_trip_id
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(polled_at)
            .bind(&dep.global_stop_id)
            .bind(&dep.global_route_id)
            .bind(&dep.route_short_name)
            .bind(&dep.headsign)
            .bind(dep.departure_time)
            .bind(dep.scheduled_departure_time)
            .bind(dep.delay_seconds)
            .bind(dep.is_real_time)
            .bind(dep.is_cancelled)
            .bind(&dep.rt_trip_id)
            .execute(&mut *tx)
            .await
            .context("Failed to insert departure")?;
        }

        tx.commit().await.context("Failed to commit departure log")?;
        Ok(departures.len())
    }
}
