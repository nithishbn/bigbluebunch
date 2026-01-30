use anyhow::{Context, Result};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use crate::models::BusObservation;

/// Database manager for storing bus observations
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection pool and initialize schema
    pub async fn new(path: &str) -> Result<Self> {
        let database_url = format!("sqlite://{}?mode=rwc", path);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool };
        db.init_schema().await?;
        Ok(db)
    }

    /// Initialize database schema
    async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS observations (
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
            )"
        )
        .execute(&self.pool)
        .await
        .context("Failed to create observations table")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_timestamp ON observations(timestamp)")
            .execute(&self.pool)
            .await
            .context("Failed to create timestamp index")?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_vehicle ON observations(vehicle_id)")
            .execute(&self.pool)
            .await
            .context("Failed to create vehicle index")?;

        tracing::debug!("Database schema initialized");
        Ok(())
    }

    /// Insert a single observation into the database
    pub async fn insert_observation(&self, obs: &BusObservation) -> Result<()> {
        sqlx::query(
            "INSERT INTO observations (
                timestamp, vehicle_id, route_id, trip_id, direction_id,
                latitude, longitude, current_stop_sequence, speed, bearing
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(obs.timestamp)
        .bind(&obs.vehicle_id)
        .bind(&obs.route_id)
        .bind(&obs.trip_id)
        .bind(obs.direction_id)
        .bind(obs.latitude)
        .bind(obs.longitude)
        .bind(obs.current_stop_sequence)
        .bind(obs.speed)
        .bind(obs.bearing)
        .execute(&self.pool)
        .await
        .context("Failed to insert observation")?;

        Ok(())
    }

    /// Insert multiple observations in a transaction
    pub async fn insert_observations(&self, observations: &[BusObservation]) -> Result<usize> {
        if observations.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await
            .context("Failed to start transaction")?;

        for obs in observations {
            sqlx::query(
                "INSERT INTO observations (
                    timestamp, vehicle_id, route_id, trip_id, direction_id,
                    latitude, longitude, current_stop_sequence, speed, bearing
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(obs.timestamp)
            .bind(&obs.vehicle_id)
            .bind(&obs.route_id)
            .bind(&obs.trip_id)
            .bind(obs.direction_id)
            .bind(obs.latitude)
            .bind(obs.longitude)
            .bind(obs.current_stop_sequence)
            .bind(obs.speed)
            .bind(obs.bearing)
            .execute(&mut *tx)
            .await
            .context("Failed to insert observation in transaction")?;
        }

        tx.commit().await
            .context("Failed to commit transaction")?;

        tracing::debug!(count = observations.len(), "Inserted observations");
        Ok(observations.len())
    }

    /// Get total count of observations in database
    pub async fn count_observations(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM observations")
            .fetch_one(&self.pool)
            .await
            .context("Failed to count observations")?;

        Ok(row.0)
    }

    /// Get count of Route 1 observations
    pub async fn count_route_1_observations(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM observations WHERE route_id = '1'"
        )
        .fetch_one(&self.pool)
        .await
        .context("Failed to count Route 1 observations")?;

        Ok(row.0)
    }
}
