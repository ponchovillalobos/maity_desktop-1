use crate::database::models::{MeetingSyncStatus, SyncQueueJob};
use sqlx::{Error as SqlxError, SqlitePool};

pub struct SyncQueueRepository;

impl SyncQueueRepository {
    /// Enqueue a new sync job, returns its id
    pub async fn enqueue(
        pool: &SqlitePool,
        job_type: &str,
        meeting_id: &str,
        payload: &str,
        max_attempts: i64,
        depends_on: Option<i64>,
    ) -> Result<i64, SqlxError> {
        let result = sqlx::query(
            "INSERT INTO sync_queue (job_type, meeting_id, payload, max_attempts, depends_on)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(job_type)
        .bind(meeting_id)
        .bind(payload)
        .bind(max_attempts)
        .bind(depends_on)
        .execute(pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get jobs ready for processing:
    /// - status = 'pending'
    /// - next_retry_at is NULL or <= now
    /// - dependency is NULL or completed
    pub async fn get_ready_jobs(
        pool: &SqlitePool,
        limit: i64,
    ) -> Result<Vec<SyncQueueJob>, SqlxError> {
        sqlx::query_as::<_, SyncQueueJob>(
            "SELECT sq.* FROM sync_queue sq
             WHERE sq.status = 'pending'
               AND (sq.next_retry_at IS NULL OR sq.next_retry_at <= datetime('now'))
               AND (sq.depends_on IS NULL
                    OR EXISTS (SELECT 1 FROM sync_queue dep WHERE dep.id = sq.depends_on AND dep.status = 'completed'))
             ORDER BY sq.id ASC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Claim a job for processing (set status to in_progress)
    pub async fn claim_job(pool: &SqlitePool, id: i64) -> Result<bool, SqlxError> {
        let result = sqlx::query(
            "UPDATE sync_queue SET status = 'in_progress', updated_at = datetime('now')
             WHERE id = ? AND status = 'pending'",
        )
        .bind(id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Mark a job as completed with optional result data
    pub async fn complete_job(
        pool: &SqlitePool,
        id: i64,
        result_data: Option<&str>,
    ) -> Result<bool, SqlxError> {
        let result = sqlx::query(
            "UPDATE sync_queue SET status = 'completed', result_data = ?, completed_at = datetime('now'), updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(result_data)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Mark a job as failed. If attempts exhausted, set status='failed'; otherwise stay 'pending' with next_retry_at.
    pub async fn fail_job(
        pool: &SqlitePool,
        id: i64,
        error_msg: &str,
        next_retry_at: Option<&str>,
    ) -> Result<bool, SqlxError> {
        // First increment attempt_count and check if exhausted
        let result = sqlx::query(
            "UPDATE sync_queue SET
               attempt_count = attempt_count + 1,
               last_error = ?,
               status = CASE WHEN attempt_count + 1 >= max_attempts THEN 'failed' ELSE 'pending' END,
               next_retry_at = CASE WHEN attempt_count + 1 >= max_attempts THEN NULL ELSE ? END,
               updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(error_msg)
        .bind(next_retry_at)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Get sync status summary for a specific meeting
    pub async fn get_meeting_sync_status(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<MeetingSyncStatus>, SqlxError> {
        let row = sqlx::query_as::<_, MeetingSyncStatus>(
            "SELECT
               meeting_id,
               COUNT(*) as total_jobs,
               SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
               SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress,
               SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed
             FROM sync_queue
             WHERE meeting_id = ?
             GROUP BY meeting_id",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Get sync status for all meetings that have pending/in_progress/failed jobs
    pub async fn get_all_sync_statuses(
        pool: &SqlitePool,
    ) -> Result<Vec<MeetingSyncStatus>, SqlxError> {
        sqlx::query_as::<_, MeetingSyncStatus>(
            "SELECT
               meeting_id,
               COUNT(*) as total_jobs,
               SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
               SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress,
               SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
               SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed
             FROM sync_queue
             GROUP BY meeting_id",
        )
        .fetch_all(pool)
        .await
    }

    /// Reset stale jobs that have been in_progress for more than stale_seconds
    pub async fn reset_stale_jobs(pool: &SqlitePool, stale_seconds: i64) -> Result<u64, SqlxError> {
        let result = sqlx::query(
            "UPDATE sync_queue SET status = 'pending', updated_at = datetime('now')
             WHERE status = 'in_progress'
               AND updated_at <= datetime('now', '-' || ? || ' seconds')",
        )
        .bind(stale_seconds)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get the result_data of a job's dependency
    pub async fn get_dependency_result(
        pool: &SqlitePool,
        job_id: i64,
    ) -> Result<Option<String>, SqlxError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT dep.result_data
             FROM sync_queue sq
             JOIN sync_queue dep ON dep.id = sq.depends_on
             WHERE sq.id = ?",
        )
        .bind(job_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.and_then(|r| r.0))
    }

    /// Cancel all pending/in_progress jobs for a meeting
    pub async fn cancel_jobs_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<u64, SqlxError> {
        let result = sqlx::query(
            "DELETE FROM sync_queue WHERE meeting_id = ? AND status IN ('pending', 'in_progress')",
        )
        .bind(meeting_id)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Delete all sync_queue entries for a meeting (used in cascade delete)
    pub async fn delete_by_meeting(pool: &SqlitePool, meeting_id: &str) -> Result<u64, SqlxError> {
        let result = sqlx::query("DELETE FROM sync_queue WHERE meeting_id = ?")
            .bind(meeting_id)
            .execute(pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Get a single job by its ID (any status)
    pub async fn get_job_by_id(
        pool: &SqlitePool,
        id: i64,
    ) -> Result<Option<SyncQueueJob>, SqlxError> {
        sqlx::query_as::<_, SyncQueueJob>("SELECT * FROM sync_queue WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Clean up old completed jobs (older than N days)
    pub async fn cleanup_old_completed(pool: &SqlitePool, days: i64) -> Result<u64, SqlxError> {
        let result = sqlx::query(
            "DELETE FROM sync_queue WHERE status = 'completed' AND completed_at <= datetime('now', '-' || ? || ' days')",
        )
        .bind(days)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get result_data from a completed finalize_conversation job for a meeting.
    /// Used to recover the Supabase conversation_id when DOM events were missed.
    pub async fn get_completed_finalize_result(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<String>, SqlxError> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT result_data FROM sync_queue
             WHERE meeting_id = ? AND job_type = 'finalize_conversation' AND status = 'completed'
             LIMIT 1",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.and_then(|r| r.0))
    }
}
