use crate::database::models::RecordingLog;
use sqlx::{Error as SqlxError, SqlitePool};
use tracing::error;

pub struct RecordingLogRepository;

impl RecordingLogRepository {
    /// Insert a new recording lifecycle event
    pub async fn log_event(
        pool: &SqlitePool,
        session_id: &str,
        event_type: &str,
        event_data: Option<&str>,
        status: Option<&str>,
        error_msg: Option<&str>,
        meeting_id: Option<&str>,
        app_version: Option<&str>,
        device_info: Option<&str>,
    ) -> Result<i64, SqlxError> {
        let result = sqlx::query(
            "INSERT INTO recording_logs (session_id, event_type, event_data, status, error, meeting_id, app_version, device_info)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(session_id)
        .bind(event_type)
        .bind(event_data)
        .bind(status)
        .bind(error_msg)
        .bind(meeting_id)
        .bind(app_version)
        .bind(device_info)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get logs for a specific session
    pub async fn get_logs_by_session(
        pool: &SqlitePool,
        session_id: &str,
    ) -> Result<Vec<RecordingLog>, SqlxError> {
        sqlx::query_as::<_, RecordingLog>(
            "SELECT * FROM recording_logs WHERE session_id = ? ORDER BY created_at ASC",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
    }

    /// Get most recent logs (across all sessions)
    pub async fn get_recent_logs(
        pool: &SqlitePool,
        limit: i64,
    ) -> Result<Vec<RecordingLog>, SqlxError> {
        sqlx::query_as::<_, RecordingLog>(
            "SELECT * FROM recording_logs ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Get logs not yet synced to cloud
    pub async fn get_unsynced_logs(
        pool: &SqlitePool,
        limit: i64,
    ) -> Result<Vec<RecordingLog>, SqlxError> {
        sqlx::query_as::<_, RecordingLog>(
            "SELECT * FROM recording_logs WHERE synced_to_cloud = 0 ORDER BY created_at ASC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Mark logs as synced to cloud
    pub async fn mark_as_synced(pool: &SqlitePool, ids: &[i64]) -> Result<u64, SqlxError> {
        if ids.is_empty() {
            return Ok(0);
        }

        // Build placeholder string for IN clause
        let placeholders: Vec<String> = ids.iter().map(|_| "?".to_string()).collect();
        let query_str = format!(
            "UPDATE recording_logs SET synced_to_cloud = 1 WHERE id IN ({})",
            placeholders.join(",")
        );

        let mut query = sqlx::query(&query_str);
        for id in ids {
            query = query.bind(id);
        }

        let result = query.execute(pool).await?;
        Ok(result.rows_affected())
    }

    /// Export recent logs as JSON string (for ZIP export)
    pub async fn export_all_logs_json(pool: &SqlitePool) -> Result<String, SqlxError> {
        let logs = Self::get_recent_logs(pool, 500).await?;
        serde_json::to_string_pretty(&logs).map_err(|e| {
            error!("Failed to serialize recording logs: {}", e);
            SqlxError::Protocol(format!("JSON serialization error: {}", e))
        })
    }

    /// Delete recording logs associated with a meeting (for cascade delete)
    pub async fn delete_by_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<u64, SqlxError> {
        let result = sqlx::query("DELETE FROM recording_logs WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
