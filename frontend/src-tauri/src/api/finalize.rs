// api/finalize.rs
//
// Tauri command to call the conversations-finalize endpoint on Vercel.
// This replaces the previous deepseek-evaluate Edge Function call.
// The endpoint evaluates the conversation, generates embeddings, memories,
// and daily scores — all written directly to Supabase server-side.

use log::{error, info, warn};
use serde::{Deserialize, Serialize};

// ============================================================================
// TYPES
// ============================================================================

/// Response from the conversations-finalize Vercel API endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizeResponse {
    pub ok: bool,
    pub conversation_id: Option<String>,
    pub discarded: Option<bool>,
    pub words_count: Option<u32>,
    pub segments_count: Option<u32>,
    pub error: Option<String>,
}

/// Request body for the consolidated conversations endpoint
#[derive(Debug, Serialize)]
struct FinalizeRequest {
    action: String,
    conversation_id: String,
    duration_seconds: f64,
}

// ============================================================================
// TAURI COMMAND
// ============================================================================

/// Calls the Vercel conversations-finalize endpoint to evaluate a conversation.
///
/// The endpoint reads transcript segments from Supabase, evaluates with LLM
/// (DeepSeek → OpenAI fallback), generates embeddings, memories, and daily scores,
/// then writes everything back to Supabase.
///
/// # Arguments
/// * `conversation_id` - UUID of the conversation in omi_conversations
/// * `duration_seconds` - Duration of the conversation in seconds
/// * `access_token` - Supabase JWT from the authenticated user session
#[tauri::command]
pub async fn finalize_conversation_cloud(
    conversation_id: String,
    duration_seconds: f64,
    access_token: String,
) -> Result<FinalizeResponse, String> {
    info!(
        "Calling conversations-finalize for conversation: {} (duration: {:.0}s)",
        conversation_id, duration_seconds
    );

    let client = reqwest::Client::new();
    let response = client
        .post("https://www.maity.cloud/api/conversations")
        .header("Authorization", format!("Bearer {}", access_token))
        .json(&FinalizeRequest {
            action: "finalize".to_string(),
            conversation_id: conversation_id.clone(),
            duration_seconds,
        })
        .send()
        .await
        .map_err(|e| {
            error!("Network error calling conversations-finalize: {}", e);
            format!(
                "network:Error de conexión al analizar conversación. Verifica tu internet. ({})",
                e
            )
        })?;

    let status = response.status();
    info!("conversations-finalize response status: {}", status);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        warn!("Got 401 from conversations-finalize - session may be expired");
        return Err(
            "auth:Tu sesión ha expirado. Por favor cierra sesión y vuelve a iniciar.".to_string(),
        );
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        warn!("Got 403 from conversations-finalize - user is not the owner");
        return Err("auth:No tienes permiso para analizar esta conversación.".to_string());
    }

    if status == reqwest::StatusCode::NOT_FOUND {
        warn!("Got 404 from conversations-finalize - conversation not found");
        return Err(format!(
            "not_found:Conversación {} no encontrada.",
            conversation_id
        ));
    }

    if status == reqwest::StatusCode::BAD_REQUEST {
        let body = response.text().await.unwrap_or_default();
        warn!("Got 400 from conversations-finalize: {} - {}", status, body);
        return Err(format!(
            "validation:La conversación no tiene segmentos de transcripción. ({})",
            body
        ));
    }

    if status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        error!(
            "Server error from conversations-finalize: {} - {}",
            status, body
        );
        return Err(format!(
            "server:Error del servidor al analizar conversación ({})",
            status
        ));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!(
            "Unexpected status from conversations-finalize: {} - {}",
            status, body
        );
        return Err(format!("unknown:HTTP {} - {}", status, body));
    }

    // Parse the response
    let data: FinalizeResponse = response.json().await.map_err(|e| {
        error!("Failed to parse conversations-finalize response: {}", e);
        format!("server:Respuesta del servidor inválida: {}", e)
    })?;

    if data.ok {
        info!(
            "conversations-finalize completed: conversation={}, words={:?}, segments={:?}, discarded={:?}",
            conversation_id, data.words_count, data.segments_count, data.discarded
        );
    } else {
        warn!("conversations-finalize returned ok=false: {:?}", data.error);
    }

    Ok(data)
}
