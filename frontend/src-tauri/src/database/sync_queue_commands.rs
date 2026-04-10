use log::error;
use tauri::{AppHandle, Runtime};

use super::repositories::sync_queue::SyncQueueRepository;
use crate::database::models::{MeetingSyncStatus, SyncQueueJob};
use crate::state::AppState;

#[tauri::command]
pub async fn sync_queue_enqueue<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    job_type: String,
    meeting_id: String,
    payload: String,
    max_attempts: Option<i64>,
    depends_on: Option<i64>,
) -> Result<i64, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::enqueue(
        pool,
        &job_type,
        &meeting_id,
        &payload,
        max_attempts.unwrap_or(10),
        depends_on,
    )
    .await
    .map_err(|e| {
        error!("Failed to enqueue sync job: {}", e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn sync_queue_get_ready_jobs<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<SyncQueueJob>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_ready_jobs(pool, limit.unwrap_or(10))
        .await
        .map_err(|e| {
            error!("Failed to get ready sync jobs: {}", e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_claim_job<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::claim_job(pool, id).await.map_err(|e| {
        error!("Failed to claim sync job {}: {}", id, e);
        e.to_string()
    })
}

#[tauri::command]
pub async fn sync_queue_complete_job<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    id: i64,
    result_data: Option<String>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::complete_job(pool, id, result_data.as_deref())
        .await
        .map_err(|e| {
            error!("Failed to complete sync job {}: {}", id, e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_fail_job<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    id: i64,
    error_msg: String,
    next_retry_at: Option<String>,
) -> Result<bool, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::fail_job(pool, id, &error_msg, next_retry_at.as_deref())
        .await
        .map_err(|e| {
            error!("Failed to fail sync job {}: {}", id, e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_get_meeting_status<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<MeetingSyncStatus>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_meeting_sync_status(pool, &meeting_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get sync status for meeting {}: {}",
                meeting_id, e
            );
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_get_all_statuses<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MeetingSyncStatus>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_all_sync_statuses(pool)
        .await
        .map_err(|e| {
            error!("Failed to get all sync statuses: {}", e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_reset_stale<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    stale_seconds: Option<i64>,
) -> Result<u64, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::reset_stale_jobs(pool, stale_seconds.unwrap_or(300))
        .await
        .map_err(|e| {
            error!("Failed to reset stale sync jobs: {}", e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_get_dependency_result<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    job_id: i64,
) -> Result<Option<String>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_dependency_result(pool, job_id)
        .await
        .map_err(|e| {
            error!("Failed to get dependency result for job {}: {}", job_id, e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_get_job<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Option<SyncQueueJob>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_job_by_id(pool, id)
        .await
        .map_err(|e| {
            error!("Failed to get sync job {}: {}", id, e);
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_cancel_meeting<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<u64, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::cancel_jobs_for_meeting(pool, &meeting_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to cancel sync jobs for meeting {}: {}",
                meeting_id, e
            );
            e.to_string()
        })
}

#[tauri::command]
pub async fn sync_queue_get_finalize_result<R: Runtime>(
    _app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    meeting_id: String,
) -> Result<Option<String>, String> {
    let pool = state.db_manager.pool();
    SyncQueueRepository::get_completed_finalize_result(pool, &meeting_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to get finalize result for meeting {}: {}",
                meeting_id, e
            );
            e.to_string()
        })
}
