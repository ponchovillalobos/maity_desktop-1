// audio/transcription/deepgram_commands.rs
//
// Tauri commands for Deepgram cloud transcription service.
// Handles proxy configuration management for connecting via Cloudflare Worker proxy.
// The API key never reaches the client — the proxy holds it server-side.

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

// ============================================================================
// PROXY CONFIG CACHE
// ============================================================================

/// Cached proxy configuration with expiration tracking
struct CachedProxyConfig {
    proxy_base_url: String,
    jwt: String,
    expires_at: Instant,
}

/// Global cache for proxy configuration
static PROXY_CONFIG_CACHE: Mutex<Option<CachedProxyConfig>> = Mutex::new(None);

/// Buffer time before config expiry to trigger refresh (30 seconds)
const CONFIG_REFRESH_BUFFER_SECS: u64 = 30;

// ============================================================================
// TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepgramProxyConfig {
    pub proxy_base_url: String,
    pub jwt: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepgramProxyConfigError {
    pub error: String,
    pub details: Option<String>,
}

/// Response from Vercel API /api/deepgram-token
#[derive(Debug, Deserialize)]
struct VercelDeepgramTokenResponse {
    #[allow(dead_code)]
    mode: String,
    ws_url: String,
    #[allow(dead_code)]
    config: serde_json::Value,
}

/// JWT TTL in seconds (5 minutes)
const JWT_TTL_SECS: u64 = 300;

// ============================================================================
// TAURI COMMANDS
// ============================================================================

/// Set the proxy configuration (called from frontend after fetching from Vercel API)
/// This is the bridge between the TypeScript API client and Rust transcription
#[tauri::command]
pub async fn set_deepgram_proxy_config(
    proxy_base_url: String,
    jwt: String,
    expires_in: u64,
) -> Result<(), String> {
    info!("Setting Deepgram proxy config (expires in {}s)", expires_in);

    // Validate inputs
    if proxy_base_url.is_empty() {
        return Err("Proxy base URL cannot be empty".to_string());
    }
    if jwt.is_empty() {
        return Err("JWT cannot be empty".to_string());
    }

    // Cache the config
    let expires_at = Instant::now() + Duration::from_secs(expires_in);

    let mut cache = PROXY_CONFIG_CACHE.lock().map_err(|e| {
        error!("Failed to lock proxy config cache: {}", e);
        format!("Internal error: {}", e)
    })?;

    *cache = Some(CachedProxyConfig {
        proxy_base_url,
        jwt,
        expires_at,
    });

    info!("Proxy config cached successfully");
    Ok(())
}

/// Fetch proxy configuration from Vercel API (called from frontend).
/// This runs the HTTP request from Rust to avoid CORS issues in the WebView.
/// Caches the config internally and returns it.
#[tauri::command]
pub async fn fetch_deepgram_proxy_config(
    access_token: String,
) -> Result<DeepgramProxyConfig, String> {
    // Check if we have a valid cached config first
    {
        let cache = PROXY_CONFIG_CACHE.lock().map_err(|e| {
            error!("Failed to lock proxy config cache: {}", e);
            format!("Internal error: {}", e)
        })?;

        if let Some(cached) = &*cache {
            let now = Instant::now();
            if cached.expires_at > now + Duration::from_secs(CONFIG_REFRESH_BUFFER_SECS) {
                let expires_in = cached.expires_at.duration_since(now).as_secs();
                info!("Using cached proxy config (expires in {}s)", expires_in);
                return Ok(DeepgramProxyConfig {
                    proxy_base_url: cached.proxy_base_url.clone(),
                    jwt: cached.jwt.clone(),
                    expires_in,
                });
            }
        }
    }

    info!("Fetching Deepgram proxy config from Vercel API...");

    // Make the HTTP request from Rust (no CORS restrictions)
    let client = reqwest::Client::new();
    let response = client
        .get("https://www.maity.cloud/api/deepgram-token")
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| {
            error!("Network error calling Vercel API: {}", e);
            format!(
                "network:Error de conexión. Verifica tu internet e intenta de nuevo. ({})",
                e
            )
        })?;

    let status = response.status();
    info!("Vercel API response status: {}", status);

    if status == reqwest::StatusCode::UNAUTHORIZED {
        warn!("Got 401 from Vercel API - session may be expired");
        return Err(
            "auth:Tu sesión ha expirado. Por favor cierra sesión y vuelve a iniciar.".to_string(),
        );
    }

    if status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        error!("Server error from Vercel API: {} - {}", status, body);
        return Err(format!(
            "server:Error del servidor al obtener credenciales de transcripción ({})",
            status
        ));
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("Unexpected status from Vercel API: {} - {}", status, body);
        return Err(format!("unknown:HTTP {} - {}", status, body));
    }

    // Parse the response
    let data: VercelDeepgramTokenResponse = response.json().await.map_err(|e| {
        error!("Failed to parse Vercel API response: {}", e);
        format!("server:Respuesta del servidor inválida: {}", e)
    })?;

    // Extract proxy_base_url and jwt from ws_url
    // ws_url format: "wss://proxy.workers.dev?token=JWT&model=...&language=..."
    let ws_url = Url::parse(&data.ws_url).map_err(|e| {
        error!("Failed to parse ws_url: {}", e);
        format!(
            "server:Respuesta del servidor inválida: URL malformada ({})",
            e
        )
    })?;

    let jwt = ws_url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .map(|(_, value)| value.to_string())
        .ok_or_else(|| {
            error!("No token found in ws_url: {}", data.ws_url);
            "server:Respuesta del servidor inválida: falta token en ws_url".to_string()
        })?;

    // Build proxy base URL (scheme + host + path, no query params)
    let proxy_base_url = format!(
        "{}://{}{}",
        ws_url.scheme(),
        ws_url.host_str().unwrap_or(""),
        ws_url.path()
    );

    info!("Proxy config obtained - base URL: {}", proxy_base_url);

    // Cache the config
    let expires_at = Instant::now() + Duration::from_secs(JWT_TTL_SECS);
    {
        let mut cache = PROXY_CONFIG_CACHE.lock().map_err(|e| {
            error!("Failed to lock proxy config cache: {}", e);
            format!("Internal error: {}", e)
        })?;

        *cache = Some(CachedProxyConfig {
            proxy_base_url: proxy_base_url.clone(),
            jwt: jwt.clone(),
            expires_at,
        });
    }

    info!(
        "Proxy config cached successfully (expires in {}s)",
        JWT_TTL_SECS
    );

    Ok(DeepgramProxyConfig {
        proxy_base_url,
        jwt,
        expires_in: JWT_TTL_SECS,
    })
}

/// Get the cached proxy configuration if valid
#[tauri::command]
pub async fn get_deepgram_proxy_config() -> Result<Option<DeepgramProxyConfig>, String> {
    let cache = PROXY_CONFIG_CACHE.lock().map_err(|e| {
        error!("Failed to lock proxy config cache: {}", e);
        format!("Internal error: {}", e)
    })?;

    match &*cache {
        Some(cached) => {
            let now = Instant::now();
            if cached.expires_at > now + Duration::from_secs(CONFIG_REFRESH_BUFFER_SECS) {
                let expires_in = cached.expires_at.duration_since(now).as_secs();
                Ok(Some(DeepgramProxyConfig {
                    proxy_base_url: cached.proxy_base_url.clone(),
                    jwt: cached.jwt.clone(),
                    expires_in,
                }))
            } else {
                // Config expired or about to expire
                warn!("Proxy config expired or about to expire");
                Ok(None)
            }
        }
        None => Ok(None),
    }
}

/// Check if a valid proxy configuration is available
#[tauri::command]
pub async fn has_valid_deepgram_proxy_config() -> bool {
    let cache = match PROXY_CONFIG_CACHE.lock() {
        Ok(c) => c,
        Err(_) => return false,
    };

    match &*cache {
        Some(cached) => {
            cached.expires_at > Instant::now() + Duration::from_secs(CONFIG_REFRESH_BUFFER_SECS)
        }
        None => false,
    }
}

/// Clear the cached proxy configuration (e.g., on logout)
#[tauri::command]
pub async fn clear_deepgram_proxy_config() -> Result<(), String> {
    info!("Clearing Deepgram proxy config cache");

    let mut cache = PROXY_CONFIG_CACHE.lock().map_err(|e| {
        error!("Failed to lock proxy config cache: {}", e);
        format!("Internal error: {}", e)
    })?;

    *cache = None;
    Ok(())
}

// ============================================================================
// INTERNAL FUNCTIONS (for use within Rust code)
// ============================================================================

/// Get the current proxy config if valid (for internal use)
/// Returns None if no config or config is expired
/// Returns Some((proxy_base_url, jwt)) if valid
pub fn get_cached_proxy_config() -> Option<(String, String)> {
    let cache = PROXY_CONFIG_CACHE.lock().ok()?;

    cache.as_ref().and_then(|cached| {
        if cached.expires_at > Instant::now() + Duration::from_secs(CONFIG_REFRESH_BUFFER_SECS) {
            Some((cached.proxy_base_url.clone(), cached.jwt.clone()))
        } else {
            None
        }
    })
}

/// Check if proxy config is available and valid (for internal use)
pub fn has_cached_proxy_config() -> bool {
    get_cached_proxy_config().is_some()
}
