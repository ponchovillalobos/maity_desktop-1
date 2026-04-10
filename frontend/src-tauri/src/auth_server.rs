use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const OAUTH_CALLBACK_PORT: u16 = 17823;
const SERVER_TIMEOUT_SECS: u64 = 300; // 5 minutes

static SERVER_RUNNING: AtomicBool = AtomicBool::new(false);

// Global storage for pending auth data (survives event race conditions)
static PENDING_AUTH_CODE: std::sync::LazyLock<Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));
static PENDING_AUTH_TOKENS: std::sync::LazyLock<Mutex<Option<AuthTokens>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

/// HTML page served at GET /auth/callback.
/// JavaScript extracts tokens from the URL fragment and POSTs them back.
const CALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>Maity – Iniciar sesión</title>
<style>
  *{margin:0;padding:0;box-sizing:border-box}
  body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;
       background:#111827;color:#f9fafb;display:flex;align-items:center;
       justify-content:center;height:100vh}
  .card{text-align:center;padding:3rem;border-radius:1rem;
        background:#1f2937;max-width:420px;width:90%}
  .logo{width:64px;height:64px;margin:0 auto 1.25rem}
  h1{font-size:1.5rem;margin-bottom:.75rem}
  p{color:#9ca3af;margin-bottom:1rem}
  .spinner{width:40px;height:40px;margin:0 auto 1.5rem;
           border:4px solid #374151;border-top-color:#3b82f6;
           border-radius:50%;animation:spin .8s linear infinite}
  @keyframes spin{to{transform:rotate(360deg)}}
  .success{color:#34d399}
  .error{color:#f87171}
</style>
</head>
<body>
<div class="card">
  <img class="logo" src="data:image/png;base64,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" alt="Maity"/>
  <div class="spinner" id="spinner"></div>
  <h1 id="title">Iniciando sesión...</h1>
  <p id="message">Por favor espera mientras completamos la autenticación.</p>
</div>
<script>
(function(){
  var hash = window.location.hash.substring(1);
  if(!hash){
    document.getElementById('spinner').style.display='none';
    document.getElementById('title').textContent='Error de inicio de sesión';
    document.getElementById('title').className='error';
    document.getElementById('message').textContent='No se recibieron datos de autenticación. Intenta de nuevo desde la app.';
    return;
  }
  var params = new URLSearchParams(hash);
  var accessToken = params.get('access_token');
  var refreshToken = params.get('refresh_token');
  if(!accessToken || !refreshToken){
    document.getElementById('spinner').style.display='none';
    document.getElementById('title').textContent='Error de inicio de sesión';
    document.getElementById('title').className='error';
    document.getElementById('message').textContent='Faltan tokens de autenticación. Intenta de nuevo desde la app.';
    return;
  }
  fetch('http://127.0.0.1:OAUTH_PORT/auth/tokens',{
    method:'POST',
    headers:{'Content-Type':'application/json'},
    body:JSON.stringify({access_token:accessToken,refresh_token:refreshToken})
  }).then(function(r){
    if(r.ok){
      document.getElementById('spinner').style.display='none';
      document.getElementById('title').textContent='\u00a1Inicio de sesión exitoso!';
      document.getElementById('title').className='success';
      document.getElementById('message').textContent='Puedes cerrar esta pestaña y volver a Maity.';
    } else {
      throw new Error('Server returned '+r.status);
    }
  }).catch(function(e){
    document.getElementById('spinner').style.display='none';
    document.getElementById('title').textContent='Error de inicio de sesión';
    document.getElementById('title').className='error';
    document.getElementById('message').textContent='No se pudo completar el inicio de sesión: '+e.message;
  });
})();
</script>
</body>
</html>"#;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(serde::Serialize, Clone, Debug)]
struct AuthCode {
    code: String,
}

#[derive(serde::Serialize, Clone, Debug)]
struct AuthServerStopped {
    reason: String,
}

/// Start the OAuth callback server. Returns the port number.
/// Idempotent: if the server is already running, returns Ok(port) immediately.
#[tauri::command]
pub async fn start_oauth_server<R: Runtime>(app: AppHandle<R>) -> Result<u16, String> {
    if SERVER_RUNNING.load(Ordering::SeqCst) {
        log::info!(
            "[AuthServer] Server already running on port {}",
            OAUTH_CALLBACK_PORT
        );
        return Ok(OAUTH_CALLBACK_PORT);
    }

    let addr = format!("127.0.0.1:{}", OAUTH_CALLBACK_PORT);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind OAuth server on {}: {}", addr, e))?;

    SERVER_RUNNING.store(true, Ordering::SeqCst);
    log::info!("[AuthServer] Started on {}", addr);

    let app_handle = app.clone();
    tokio::spawn(async move {
        run_server(listener, app_handle).await;
    });

    Ok(OAUTH_CALLBACK_PORT)
}

async fn run_server<R: Runtime>(listener: TcpListener, app: AppHandle<R>) {
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(SERVER_TIMEOUT_SECS));
    tokio::pin!(timeout);

    let shutdown_reason = loop {
        tokio::select! {
            _ = &mut timeout => {
                log::info!("[AuthServer] Timeout reached, shutting down");
                break "timeout";
            }
            result = listener.accept() => {
                match result {
                    Ok((stream, _addr)) => {
                        let app_clone = app.clone();
                        let should_shutdown = handle_connection(stream, app_clone).await;
                        if should_shutdown {
                            log::info!("[AuthServer] Tokens received, shutting down");
                            break "tokens_received";
                        }
                    }
                    Err(e) => {
                        log::error!("[AuthServer] Accept error: {}", e);
                    }
                }
            }
        }
    };

    if let Err(e) = app.emit(
        "auth-server-stopped",
        AuthServerStopped {
            reason: shutdown_reason.to_string(),
        },
    ) {
        log::error!("[AuthServer] Failed to emit auth-server-stopped: {}", e);
    }

    SERVER_RUNNING.store(false, Ordering::SeqCst);
    log::info!("[AuthServer] Server stopped (reason: {})", shutdown_reason);
}

/// Handle a single HTTP connection. Returns true if the server should shut down.
async fn handle_connection<R: Runtime>(
    mut stream: tokio::net::TcpStream,
    app: AppHandle<R>,
) -> bool {
    // Read HTTP request in a loop to handle TCP segmentation.
    // The first read gets the request line + headers (and possibly partial body).
    // For POST requests we continue reading until Content-Length is satisfied.
    let mut buf = Vec::with_capacity(16384);
    let read_timeout = std::time::Duration::from_secs(5);

    // First read — must get at least the request line
    {
        let mut tmp = vec![0u8; 8192];
        let n = match tokio::time::timeout(read_timeout, stream.read(&mut tmp)).await {
            Ok(Ok(n)) if n > 0 => n,
            _ => return false,
        };
        buf.extend_from_slice(&tmp[..n]);
    }

    // If we have a POST, keep reading until we have the full body
    {
        let preview = String::from_utf8_lossy(&buf);
        if preview.starts_with("POST") {
            // Read remaining chunks until we have headers + full body
            loop {
                let so_far = String::from_utf8_lossy(&buf);
                if let Some(header_end) = so_far.find("\r\n\r\n") {
                    let headers_part = &so_far[..header_end];
                    // Parse Content-Length
                    let content_length: usize = headers_part
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_lowercase();
                            if lower.starts_with("content-length:") {
                                line.split(':').nth(1)?.trim().parse().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);

                    let body_start = header_end + 4;
                    let body_received = buf.len().saturating_sub(body_start);
                    if body_received >= content_length {
                        break; // We have the full body
                    }
                }

                // Read another chunk
                let mut tmp = vec![0u8; 4096];
                match tokio::time::timeout(read_timeout, stream.read(&mut tmp)).await {
                    Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&tmp[..n]),
                    _ => break, // Timeout or EOF — process what we have
                }

                // Safety limit: 64 KB should be more than enough for auth tokens
                if buf.len() > 65536 {
                    log::warn!("[AuthServer] Request too large ({}), truncating", buf.len());
                    break;
                }
            }
        }
    }

    let request = String::from_utf8_lossy(&buf);

    // Parse the first line to get method and path
    let first_line = match request.lines().next() {
        Some(line) => line,
        None => return false,
    };
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() < 2 {
        return false;
    }
    let method = parts[0];
    let path = parts[1];

    log::info!("[AuthServer] Connection: {} {}", method, path);

    match (method, path) {
        ("GET", p) if p.starts_with("/auth/callback") => {
            // Check for PKCE flow: ?code=... in query params
            if let Some(query_start) = p.find('?') {
                let query_string = &p[query_start + 1..];
                let params: Vec<(&str, &str)> = query_string
                    .split('&')
                    .filter_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        Some((parts.next()?, parts.next().unwrap_or("")))
                    })
                    .collect();

                let code = params.iter().find(|(k, _)| *k == "code").map(|(_, v)| *v);

                if let Some(code_value) = code {
                    if !code_value.is_empty() {
                        log::info!("[AuthServer] PKCE code received, storing and emitting auth-code-received event");

                        // Store code in global state so frontend can poll if event is missed
                        if let Ok(mut guard) = PENDING_AUTH_CODE.lock() {
                            *guard = Some(code_value.to_string());
                        }

                        if let Err(e) = app.emit(
                            "auth-code-received",
                            AuthCode {
                                code: code_value.to_string(),
                            },
                        ) {
                            log::error!("[AuthServer] Failed to emit auth-code-received: {}", e);
                        }

                        // Bring app window to front
                        focus_main_window(&app);

                        // Serve success page immediately for PKCE flow
                        let success_html = r#"<!DOCTYPE html>
<html lang="es"><head><meta charset="utf-8"/><meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>Maity – Iniciar sesión</title>
<style>*{margin:0;padding:0;box-sizing:border-box}body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#111827;color:#f9fafb;display:flex;align-items:center;justify-content:center;height:100vh}.card{text-align:center;padding:3rem;border-radius:1rem;background:#1f2937;max-width:420px;width:90%}.logo{width:64px;height:64px;margin:0 auto 1.25rem}h1{font-size:1.5rem;margin-bottom:.75rem}.success{color:#34d399}p{color:#9ca3af;margin-bottom:1rem}</style>
</head><body><div class="card"><img class="logo" src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAYAAADDPmHLAAAll0lEQVR4nN2de5hdZXX/P2vtfc4EQsJFrgJCEW+obSN3FJAilgKZJOCAoiVqW/vUGyBoLQYmQ5Bq8YIo+Px8Hi+oWMgASSaBIlUwgFbuP1tFigr4QwFFTQgQMmfvvdbvj3fvOWcm58y5zkySxTNPhj3vfvd7f9blu9YStnFyEEAEzDlhNszuN/xU4GDgJXmxPyryAMgKiFcJw887KOACPmONnwaSmW7AVJOHyfeEU06LiS8A3hDm1iaUDM8M/q/CvworlxfvTn+rp490phswVeQgzqAKeEb/p2PK14O+wXA3UjPcnfBf7TNF/xKi65z+z4R6BtW34Y2yzXYsTP6QOQsugdInIMkMF0EmXfSOG4grpciofDpi5ONFXdPV9umkbXIBVCd/4d8AN4GbgUqL/fVw7JuiUYr1l1i12hmIhOFsals+/bSNLgAEju0zdrpDiQ410kyQqL06PFPiyEgeVGa9EYY3bYv8wDbHA4SdisOcYwQ9FFJrd/IBBImM1JV4XkrlOAF3BtquZ0unLWoBBMYNzRmvLpmv+K1ChHW3aw0iV/jrTivobZ96T1vEAgiDE3augAlDFv7Fw98GW25ncU8b9rrenNguwEG1dbf0Vg/7NJUUz+THw24YlILDfoQT+17B9nuCbQ+6ETY+LQyNhrKDCkOTKmYKuX0wLOyd86Ld7Lj8c76Lc2wsrE2b6QZ63aepphk7jmoH0ll4MLAY/BiDl4HPUmQT8ARwB2TfFNbcO/G9RnWGSej/IcRHWuABOtptjpsSq5HeewN9R57OcNbK98Pv/YeAnjWxT4b8RvG1Cdk3yy30aappRhZAdaIWzzLWLQP5oFKaFbRzteK25j/pKHAVpEuENRtbWwT9K6C0wEg6YgJDXZ4FfUCyJmJkfmvfHdgOkmXgH2CSPhnpqOJXQnZhsz5NJU37PeT5ser0zzHWLVfK5yvMMpK00MZVf1IzktTxPojPNeJh551zw/veYPEOKEAG94cx7XaRC4rfX1v35H0aXQ7xeTTpk+B9UPoIRMudE+eGehr1aepoJhgREcSBzyvl+UYltcAoxYKohFnLf0QFYgc3KqlSOsnY8MXw/tIGgzUMQES2GpJREPEOdpaAKyJGkoCtnrz0oAjihl+ulE9pp09QPtkoXxF2f6M+TR1N6wLIuWJzFpwE0d9BkoFEzTR04e8SERbBWc6ChUHTt7lcLpAFTeBNPzH8RiUO5oA2yTCDWIGVwk33hzo3lwJCn4YsYcF8pfRea7NPxmimxIud/v5GfZpKmuYTYLk5iOEfDveg06p6VsJ7CmD4MmdgBxj2RnK1gyjZEiP9nRLF3sYiKLSAkD6j2AWTfSO04YTZii/Ln0o7fcp/c4MPB6lgeFptDtO2ABw0HN3zXw5yFGTQJncuiBqpKaXXQeX9AlbvXs5FMBFuelTJzgI2KHHkkE52HXiQINIw+TwHdpaw+pehrnrGoEEJbZj9j0rpLzqTOEQhFZAj4CcH5lLMtM3LNJ4AA8WueLWic8C91Z2yOWVu8FFnwf4wbPWUKuGqGYiENbdCZQH4L5VSrIg4ntX7UUSUUgz2aEqySFh9S3FtTay/kOGdBfsa/s+QdcTBh5MNV3R2ih8Ung5MGy8wA0yg7BKONO9wwEQNM6W0q+FLJhOdhOGCH/jBRipvNipXGbZeKUVKOQr/Vn837FlIvgxybIk1329mARRww5copd0N61jfAO6OIMgunb3fOU27JjCDShT0413IZ6JGYor+bYWF3xCG7mo0WYGxGlRh6LfAB5z5n8+onCDooYrvkZf6XYbfH5HdKtz0CxgzKded/Jzxy5z5Rxi6GBJr9zqb2Mp8LCqd19EZTeMCOMgBIuT/GZlpFwMmIAG4EZVLpJc47zsB9mqopQuLYExF+0vgl8CX69VdVc/WB4CEeg5y59gY5BJF+3Jzczf9USPzCH4dnhw0bQqhabtrajRlOxiVB5Xo5Ubm0sU1VKhqIV0sjHyzFdBGmOCHJAzyUD7Qg1I8a4b8Kb7hLDgTomu6UTWH+jBFxcgeV/rmCcPPTqdWcFoVDzUwrWVQWgKVzDtU04b63JRIMrLHIjYdCreuqzXE9JqK0wEW7GhwjxIdaKTe3e73jKBunhH42QypgpMrofIERBqO8s4oMISZRZQOMGadN/W75qF8Z9o5SvyK7ne/G0RqpE8qXDETWIFpXQBhZQ+ocPPTIJ/MdfXd1ipG5op+cBP9BxVMX/f1jqdCSeMsfCXoOUbBcnRLIor9qzDyZBib6QWfzoAYOGxB0TH6DSO5W4m1HS3dRBJQxxyiuSVkaQ8bWu9bbthSiHd0rCf8i5HcB7O+GsZkerWA0GEHuoE5hSN0QIRbRg1ZYlgG0hXTEyFipAZymjP/xF7r1At9v9P/FtDTjdSiLna/5Ddh0GfoEmH4xTAmrY9Br6Bmbb0UBrU+p1zlroetlY4UzE5G/3eU0jsykq5Ew6pEkN0H5WNgeBS6d+0qXMvgxJLRt1aJDu8EZVxLhltESY10OGLV6a0yfqEtA9psDtqBrrWkB6jKxqFiZ/EsWL8n2GzQF2Cnp4WhTbXlW++QLoX0JEHnOm6dHqs1doJDIPk7gS8tZyCiayz/gArDWUr5PRHx4d0ATCBwkIIKZBsUGWz9vWJM680BG2GXp4o5mAhLm4yangC1k+ks+ivI3gEcYbAPMAvYBPwW5G5FrhNW3gqwnIHo9KYyeSFT9w9CaamRdLWzCrHQ8KcUOxRGnupGLKyKffN3N+Q+Jdo76C66OqkypRRB+klh1ZJWNsv4OZh/giFnAIczNge6Cfy3wN0K3xFWfX/ie41o0hOgKrfP/zNDLgM/DcqAoVX7yCzQnUBfC+l7MxZ8L8P/pczwfc0bcFBuzu273Ki8U4kP7Ea0ysXCTCm/1Kj8SwQf6la0CmAC+bhS2iff/V1eU5Ea6a+U9LOttK06B6e8AeJLgb9WYiZAzWZBtFPNHKxQRj8qDP2q2Rw0bECNe9XBwDDEf2YkDmTgOl4EclfEDCIlFsOec9L3x6z+djPt3BRo1lzDvT8K8THCDS0sxHr1FP3vn2foXQG21rqtv36d7Wkui7+nLDgzQq6CaEcLFm0jaMTHzUEwLBIpJYH018Dpwsp7Jut/A4zbmMZuf/AbQf8s4NiQRjAnhziYNtNU8TlC/HWnf1FhkWs8LIU5t3ydkX6/B2KhBEtjvJ2RLiu+3clJkINXlinR9hbEvm4mP8sRxmvh2e80A38UCCSnvz9CvwHsmJFk+XhHjaBmof9JCrqf4Tc6px4wmW6kwcQsdWdQDb8cSi8z0lRaZBgFYsMyhdiQK50T9wl28/oNCFx64FwVX2LYqHaI46vWKVFuLTwRHjwtrP7BlievEPugf6FSOrkHjF+uNLJKhi0R1qZVrWK98gXWoP+lhlwFUgpj2lobwhykqVLa28iuCNLb0rrf2mxSQufFUx44Xon6LZg62+p8GKwsVUp7waxzmoliVbv96h+DXQ0lpQsVcW3NBkNO/5wwoM0ljCrMa2AHg4u7bwOAm1JSw75VZs1djfCF41uOG5ytxHt3InYqEhkVU/RkSE8QpK5vY50BCaZIRc6CSDpF7hiiAbmTvcMZ2K1qkm1MAccnlxjJ7xXtgZ0gNaX0mgz7cKGAav7mgAYEUOX9Sul1vdD3K6pG8gdFLmllDHJF1i7AO8A6Ujnn3/HcB2FxeLq5mVknvpQzPnMUOSLH7XWqYVLDUKKXwqbDw9P6uPpQPhzTwqonFPl0WHzdUxDbovOchS9vxo9U790F+wMf7RTmtTlFonCZsOrx5vr+Yowqhyq6Lxidq5xzyCJ6uPOWHettwgkVF/ek7gO8ND+5u5gIN4jIkNe0Vr44pl/4P0byk5wh7OoUIMDHdjayi1p9z7ALId61O5jXOH3/T6F8VTv6/gw/CCKsu6uwgDO+tMLsl4VHg5MtgIJsLvis3lhXhQjZubWShZ3gP19Q5MJcvOz2+0pgCN/h9B/eyE7gUOj7DwF9F8G20BNjmcJFwvDz7ej7I2SnXlmHBfrKRHMbtK0uVQzpUTgUB2Rjq6WrDOGq1UY2Qu/EwlKGHN+45EDxy3FKXLauUMtjGj81kptgZGUrjF8tZfiLvYI3hPFLR+v9bcICGCpeeRL4U7crUPNNneGPdvJ+il8E6fOKdiUWVsmactIZdG1FDMooFSPbqMiFnRmk9NGgce3etxFkHfBk+P+hcX/VCUVzJmH17xX+J1gZO7uDwoSpQPZchNwdng63NBCF4qKP1f8NfBkipWMYeS5ckGYRsrZZecPvNNKMoFjpcNG5hzbbV4SRB9vTRA7n4Fm/x8iepavF75bP4U+XMu9pr3KFY1TnChgoOn59d4gXN4jE4LvCylwnvbmDxaQ1gED5MiP9tRJJJwxhiP0XC7ACRu5sdBQXV0+Z1T8Eu16JJetg0VUNUslvFP9UuxrIsAkHNUgM3JKL4l0wgiIO1w/laKyJf93swdKxXTr670by8+BS1d4dXNV8ZaOKf6ajZo/Bx4afUbikQ1l47BSCeGmrO1opLTXSDRJ2XweDr+LopcLq33UD88rgM0a2qZNToCbK2SMRfd8OT5voAQCGxlbgLRsMPcewpB2lTN7QTClphl0qjNwdZqGTQSjgY+VvGumP2rcTFLsxu1K44WfNLWNDuTvZjQ8DV2iuCGv5a7m+H9K7I9Z9vVOYVy6va5mR+xwuDnWStboIwikkCpYqdk6AmtfvewP9fBiIEitvVewDwXctUp/EudLBHc8ERCnFRuUrEfMuyRUvXdzfAyIMV4x4ieEpLdoJqqbX5HEl+kzrR3EwUSt8NphtW0Mu57yGGJal2IXC2k3twrwmdsEZ1IiVnzIqVymlWCD3a5xsDkiVSPOp/bCw5j+qto267Z6kBfmqSeh/m6KfV6J9ICPDXcYPiiiqQXGRVRQuhZVjevTuYVlj8LFvKaV3tQIcqTG9vlcY+Xo7kT5rTNR/C9E3W1EHFzAvSP5dGDmzF/j+YtEKeMr8C4XoE0rUB1kev2DcuOZzEBNg5tm5wurl+R3YsB2TdqrgxkuMXK/40ZB+Cfx3EZEEh8riJ1bDN0KyUvHjhJVD1Tp6I8yGXalLjWR9syupxvR6F+z57fb97gsT9fp/N9K1za6eKswrfRZ0abcglIKKsXOQmNXLMtLjjPTGDH9BiXXiHID/3ki+rCRHh8lvzni31NDxkKST906JjozhIJCdDDYq9gvI7hFu/vnE8r2iGvjYEigta4TOyY9HB8ky0hPKrFnbSZzf6vfmvxGi2wyPyW3vm5ctrH2ViyNWD05FXOHxc9D/qgw/QtBXKGyf4c9GRA8B/yWs+M3E8pNRGzZyFAaZrNJ2wIjtUhWfd+Ico3y3Er+63tEcdn85gsrVwsi7u1mM1Stw/ldj+t5rVDa7eqq8RvYLRQ6DVc9O9RhMdqoWxq5Wv9+yd3A4SobySa5n1SugykM9OfI3/37BoQ9vcPqvDWji8QMRrgVVI1mn6LJeHMWhDl1mVBYouksdA5FDhGLXCavWT6VvX/UEKCD4E2nY2v122+7hYfXNXNj00PkHfpNfbRMHwQMHPPpZYc2vCj/+Tr9Vs+geT1lwGUSfCsrSzVuVwW8aT0xvqZcLrCWEzJbyAzurMGSGvjafe6+2c8z0+jCkXwzle+FnP2wOElG+EpKfNjJRC/7aMDGP6kyP0/gxm5zqFmjXy2c6yTn1FUZ2lyC7e81xXBX77IyQ76d3R3ENQvpU0BtqeQ/HPWgM/Q9KdHSuRNpiqHpl1/cmks0LjxfdeiXSdEpLKTw+jy3Djm8CroDoNbmDRn4MjIV0vVXpO6kI/tBLETQAKZa6sWBNCFhZ1UU47kFraA+DnA073gFXV9gCNk+duRxnEJLqH8eJegdDdLLBIcCu9AgY0Tm5g8xVOChg3Gonv/ADkArIMcKKe6dGDC1OgUVvMOwuRfpq/QQ8NwGDY/jPwZ+lJ+7jXbXaFP0DZA+ArBFG7qvtC4w1fsz75GVGfIliZ0CpnFcyU63fjJyM4D9YXZA1Yt9VwsgHppILr/GX+AKUPmxUJrbFAj5fmeGDs4ZCO4w0AZZX2LRkO777eNGXuPhlNLgeXafEB0JCCHlaU8MWQSITBzxH3D6tVP51Oq6rXCz8lJG8TYn2MrKxRZA7yLj3DEzaE3IARUpQeucs5MgKi94uDOUnJeCcup9hdyrRvsGrpHms2y2BCg0cJOcII1+Yjvg6VXet+R+MKH2xW9j4dFHQkHqmxLGRPZkzrI+KgxgLliultxmVVJAZzSLSKtWIfQ8q/iZYswmmPtVrwUjBQJ+x6Q6ldMjWsggAHE+VcgyVlTDvNE1ZdJzCQjrwAJpZkgLtc6GwZmOXptc2vjpmon5R4cJglZOeGb2mmjR3mwOZD/e/RRU7C6K4ncjdM01V4IWtFFbc1C7itluqcWW7BfyG3Bq6VSwAH5MMo8jQxQq8MTyYaZGlNcrFLYX0hRQGZ7o9ii810ucFVe8IPjYTVByW8kYF3yt3P9oqFkDQCagYdnkfq/5npvL6VmMQjzyk8Nkg+m0Vh0A+1wb47lsF4zKBCrFmC9htwfDTCXp4SyBFXIGnQQsgxVZAoo5hyEec0147VYEhm1HOd5hz6qsj5DwnD9qxFZDn8A7Dn1FFfhgavnWs4gCMNFPi2UYy1PyNKSU30iGI5wTD1JaRibU5FZl25EeS0P8WRW7JXZBaMiHONOWeErnjaHpKFfk6PZJA8a2EU94aE99suOhWNHaE8UPhJHFQWDAMpVO3TkVQer/Sd3SvAkM2/26hCFpcNtatVeLDtk5FUDIC5VNz1Gj5fCP5rRLH3iSx0pZCNRFADobK34d+tB4HqHMKEUQS1r93a5r8YKPwNKiCk6fAzhOGM6kJh3YI6HUQH+Ak1MCgt6BjTcahcnNjkBj+tOKHdBsYshlVgakn7WHE99YLHBmupy2Kn3KAEFmsBGSPg59RhI+La3Lq3Pcif318me0+CZwejglnyzkMPFjaa8zBeQKpTCnvZSQ9CQzZjIKvWPzP9QJHBqiYaACHbCn7Jr+xSFJIhsE+Iax+rNj4DQAhpxxqSL+i8wzfjQAenamVIASZf44irwo2gGrMvhpAyCjYMcJIR4Ehm1GzwJFVVJBj2CMgG0LTZnLcSIE/gD+YIavLjNxd2xeoQQUXDomA56nai9TmCsduAXfcnDJER4NfruirC1RQWB1uSrydYcucwZMhrIheM4TBcsrFIXBkktXiAnNI2COQnaOwFp6rwPMCO8zgEbrWauFfBbStdnNshaDQ/leB3OnIboa5VnGBpsSakZ4eMzLcW1DomK/gQkdXeA0IpAYU+kfFjxFGHurFN3tFbYFCG1ewpdD7YuErScbCzyvROROAmblYmD2k+BEw8vxSkKEuDTQ19v/tjdEfKfHrJ6CCM6UcGcmXIlZ9yDm4BPen3fe1N9RsAzc92iWXrbeEH1hnzqCmpD8DH9f4qlgYHwT+QQFf2pMUrEV+4NF/Ukqvry/2OQ4/DSfnATbT4zR+zCantpU+1SNlIjXPudcLEoYspX9v8oD1m89w5qDnOQPXwvDj3VwF1Zi9p+5nZB/Vhlg/IcL37nWqmsnb1cg1rL0ru+UFUEx8ULfWV7mOL9Nb8jzaRoj7K6fXcw2rioWllxijF0Xwnm6vMAHPSJcopd0axCWQ4K8vZzgDn4XhDVNlog6La3L/v2qZ1hZCS4MzXkRc+BLwwzN4veC7KbIJeAy4R1j1P0X5XjpmhDoLMOYpn4jou6QV93DFjxdW3dmNe3iFk46KKN2uEBuTu4dnJEtjRoZ6bZeYyL07i16XkR0myAEgsxx/JsJ/Cna3sOYPoUyP3MOrHPAJuxvbnQ16piL7Mw77bhjZqMJtKfaZEqtvKxrei0VQ1cAtPMDwexXZKegC6qtgqwGS0juVPY+HvbJ2FmTVE+gHasy9VSkdN1nEbgdTRAzfoESHwo2/7JVGsnYMnVOONfRjwF8p8awqC+eEObBfK3INVL4o3Px0KwtxUiawxup1PMy+QylfoMj+hrmRZEYl/0lNkT6I/kbR72X0X+YMlIPWrHfRMozsohD3d/IYviFfQGpKfDQ8dWYRcaz1rxWRvea+PZ/8SfMFSAiMbUq8o5Es7dXJV0y+c3DJ6f+UEd2mlE5SdJaRWnX8kyyIxLofxBdA+Y6EU94aNu7kPEnDyamJD7QoJvoWyOwsz5Jd/xjEGVOFlgSSa2H9e+DNlW6ug+pRvPDYEvyn4RENjuLx741FCHtM28grPCE/8L2KvryVRFHF1aOopdiJJVZ9v5uroHoKPRQbo19XymeGlD1hjBvNgeMWEUeGvWgkZ5W46foOUsYwli9IQ7qS2Uaa6SQOI0JIZQIQnEvKb0+Z++l2s3VsPgjD7gyUS9gloCVajOFb5BVWSgfQVl7hsfzA54YkVllL1j4pzARIrPglzol9oe0dn4D5Yh29VOk7M4xpYdRpPAcB9p1mimynlL7mLDq0Rsu7GdVjogTwx1g8C+wKJZqbkbWcsSJvXASVTIk+5Mw/oXPxqEjeMPouiN/U7Ciu0xoxMjf0Q87C1zSDj1XzBSx8JejZ7eYHrrl6joBZi0Pb27l6inYMRALm9B9n6LlQyaB1C1NoR5Yp0RywK5yB7WhwHTcMFbsv6xdAfBQkLeeqqTYAMZxwBMv5YdCXtxkmNkT22sApuwIX0oGJVfK8wko0x/ChVt8zfGl3+YHdDVviLNq9GnGsHVpuud3h/BB/qH2fjbBRkgyiIzJGTw2nWguhYouJiuBMkC6C1IoGYxTHwIOvDjlr2htMAZ+NfhRK+7d6FE+kPK+wK5xaYcHRjfMFFPmBFx4JMmCk3kl+4PzqcaW0r5H+c7u8T7h+xaH/lSBvdlLoEHAS5k48zCXU24QTImwhYaJO2TXEBsgDf3ZA+Z1oSjwrC3EGaC1nT5UBdRa9DuT9kHWcvMHHIIRxVMKPbeGNY5QoBm+a46gxiUBmSvSPzqK/aA+5HMYog0OUaHsJ6XQ7bYfmc/gGZ9Hu9TbhhEYVzFppb/Bdu5VmrFDgwf6dvZ9erMQ70GXOvio19yXI6gaBavMrIBZS2s8Gu7izhST7BexDtyKlY7Ar+L7h/8c7UzVYldkskFJ3H66SILNaLVuTqv1kiBYaqXmXOfuCkiZLwW5rVj7Cb4e00qvchY72w8L+dhlhgb5eGWIFYtC6c9BgAegG8E29aYDj+LrWShZi3ynbG7KsCLnS5feNEEb1WmH1j5vlC8jBMNfk73StyZPgxXyx867Z7YiFjq/vlSbdYRSyDfX+NmEBFEEeS0+A/LaKuuqYFIwI/re14oXpNXqfEs/rRc6+PHDkesValgKgvMxI/9i73IXxn8Nz/yRtIJcdHoEM7Q5xXFzBT8Lor8Oj8YE8x1UeNBmDGjJccWeOEOtQg4cpCmS/f4Ho3vB08ly5wfS6aB+Dj/coZ5/nmUY+J6z+5WRh06GAxQ1EwvBjIdFFb3IXhgSafGwjJ+03WRrdQMM5Vi+617CnQfCOQS3uIQWS3yXcklspabwAAgU7syJfy3PndHgXuodcxqzcgRVP1fv4RMr1/RcopT16mLPv4YjkC+HobSVw5HDB/V8J6c96kbsw2AlKu/URL2llDMJYrfg9cGMYw05S1wTPqdxO87XwdHMMwWYDXL0LV94Fdm2IwdNuypgieFPl2VH4XLN7r9iZFfoPV/Q9ebSSXgBRRZGlwi0bWrXOhQkaFGHkOXoWf0A0j8pxlnPSUa0whA6i6OfyuMcdLELPoBQpvlxY9YNGvM8kalFEyc43koeVcsseQ8EUK4CKwkdmMfK/kw1+sTOdgSiCZaCzepezL70VytdXDTytUnFM/+UKI/0PpdR17kICcrhsRJc4x8ZFZpL65Ys0uit/5ci5ue1LWmlDMAiRhqwtySPg501Wvu4CqDbg5qcT0v4QiKkUa94ID0oSH//jWWGHJ0Tw+Jgw8rXmwIRicYwOKPEJ7ev7Nx+AXOzbpOiFxapvR5woygpDpkRLjPTFbnMXVu0EpeNg7tubmagL5VHMyqsz0vMVpUjgVaSNmTD+lm8+UUoxpD9J8H5h5MnJ5qDJ0Vxo5E7eGUoXgf89lHYIPEltfUJ1LdlDKdmSEiMrvEm6kurOHJhrVO5WolcaaVPT6+RtLlC6lauiLgNHNgsM2UHbChP1o4oc0kpugWIMnf5+Qy/VsUDZxvj1WAB0shfAvwrPXyx8/4/N+j9pZ6puYzetE1aeC3qkkXwS7EeGP2P4JsM3GtkTkNwM2fvWY29sZfIDFabXyoeV+FU9TNX+lJJd2gswSqhDPxWcZzvLXVhQjYn65SAfCX2fPLx8mHxUGBlRNh2Vkf0DpGuM7Ikw9r7J8Gcg+y9ILgU/Ulh1diuTn9ffnDbHpKHQvyf4zoHZiJ8RVv6xWr45EKK6+0890EjvAd0xt/V3uQBKmpGcHTNyRW8SN405hbwfoiu7X6Tk0EJ/TvHDYOSR1oAq48fUGdgFXtwd+iJI1sG8p8cnlGgNhNPWDingyPUmNyySAW0VkVrNBDb/20r5na1kApu8vrF4AQ8ofW/qVbyA/BQRWFyGdWvpgUt4kWHMSK6LGHl7qwu12RgHyaI9eH4XSB2oarXag3xVd9WivwK+a5jSAsyrEeXmvimLGFJt78K3Al1HBKlpr6ekJ5VYc2u77a1CxqCQcDpZ7D3SdLVO1R11Ysko367ER06GuG2tziJrV3JDxMjbptI7OKP/WqV0RkZi3ahpa06se5Sdjy3yC/TA+NEWzYDXb4B5ZZQW55PftdhX5AdO8R4pbhp+S5S4y7zCgWrsBIdlrH9vp/CxbmlaP1jAvJz5ewj6iU7tDBNqHcsP3MfqpvmBO6VqMusbH1bkC+3mFW5M7gIXOIv26gw+1h1N+4oL9598QIlfRocwr4Jq8gM/1l5+4E6p0N7554yk5bzCjSj0PTMl3tuwD0338R/aME2Uw2vdGdjBqDyg6IHWMeiyqLNIFJW8R1j9jdbEz4KTDnmFwtNBaTUeQk1G0XdB6Vu9EAsVlYzs8YiN84TvPVuMVad1tkPTeAIUHGvyeuDlHsKrdKnxi9VI74S9rmmWH9hBCoukMJzl8XFyN+ohC8+CJW7yk6Q4pjdcayS3t5/SfjxJQC6LIPsnzP7z8HQ6op0FmsaYgEVcXdsvCpE8xqJ7tEueHyiGJYp8QvhKUmDp65cv+IIhd07cB0rHG3KkInsDGP6kw48j5HvC0BPj3xlPYZE8pMLa1DlpiaG3g8QeFlOn/bFwndh+wJ3TkXyyoGkPChlBmaq402FH3UKiqOTq4P07qI0yhBbZQ5237mJs91Hg3RDtqTXRvYP+wN4H9ruMhVcr6WXC0B8aXSlVk/nQjzLmX630/YNRyehYmhk77cudvd85zYAYqH8Cl853/xjW4I9QnjQ/cE3G8Xmw/e1K6eMge1YdK5Os6uSaGrCHEn8M4tud/kOaOVfmJvNPGskz3cHHwpXv+J86e79zmsYFMJwv8+xhI3u+c6QRQCQKnxGGH6t68o6noBwYzpyFBxv6HxD9uZGkAWsgGnzsxv2o4R588KLXGXqzs/CwUEc94MyYyfzXCv/WKXwsmK8RwzbG+M/D0+FpkwambQEEq5YLrP4V8COInDZ3TM71i5H8DLgyRxHXmfzBHMx44m6GfUeJ9jDSVAK+quFESdiKsZGmiu5m+DXO/D3ImcPN3xjKHS3mfNlI/7sz+JgbxK7wY5j3i1wCmLZcCNN8BZyuAq74F/NrgFZPgRzoYYAoflGAbA02FJfC8/KFSvmVRpJKG/xOWARZqpQONGRw8m8MiPDtFxS/EEIii3b6lP8mwBfbj2PQPc2ELSBXBfd/XSm/26g0zVMYBspTpa9kbLomYvW7Gpk8q/qG+Qca+oAiO1gHZuZc0kDxjeAHCyP/2wjjUGMn6LBPo99WRs6ayjjHjWgGmMDgpqxUzobKGqUcS7CspZtDzdwcUgFRyiUj+a6iHwxH/9IGhpOwgzKkX4nndKpsCm0yh3h2hizI624woUvdcVHmnG1Ubm6vT5VblL4PFvW0285uaUZUweHfWzaAnWEknxdkVIljJdaAaSt+Ys1D2CeQXqGU3iasWh/elwaDNZyHD5MjCd/qAscX3o/giNq6Ny8XLMPCNRsUGzDSyx0qjftUih0SI/mCsvFtwvCzk/dp6mjar4CCatWdFRYeFmHvVuRog32BPmAU+A1wZ4ZdXWb1jye+16hOBzH671Lio7pR1Y432c47Mo+00fT74feFh9nmfaooPAHcCf4NGQvePH2q34k0Y9lBionK7717gHucxbOUP+0F5e2hshF2eUq4ehMUnH3LwBOhp32TGH6gNOHOm/dp04uw/VPC8ItADq2bfgxALc1oepjQ8aqrlDBUxBwco5q/tejUEUTODNbRvW9joWf6k7A2rf3G5G3oXZ+mmmbsCqhHVbTQIDAEHeyOQvuX0X+ZUjq/G6xhDcT88oiRczsLONl9n6aSZkIKaEi5dc5yS11XoeqV0k15kKeO+xhwfykKN3VaRy/7NBW0RS2AXlCuuhV4yQ/B78yx/G2ba8M7sQr+Q+i7IzBq05egerpom1sAOanwlUTxCw1Lc7tDy/dtMM8KhmVgFwnDlem00U8nbZMLQCA3147c4dgFSqwBeEHTRA6GZ0FxFEeKLRFW3zZTCaqng7bJVQ0F8xVUq86C8wy5VInKkGK4ecg3VPuGB5h3DGQJ+BJh1b9NReTzLYm22QVQUI2D51HgFxm8JXgw1yoJA6MeAmLwfcWWCWvu2pZ3fkHb/AKA8fCuEH5VTlP8TYa8DCBo5/wuiG4QVtw28Z1tmf4/NRem0/M/46UAAAAASUVORK5CYII=" alt="Maity"/><h1 class="success">¡Inicio de sesión exitoso!</h1><p>Puedes cerrar esta pestaña y volver a Maity.</p></div></body></html>"#;

                        let response = format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\
                             \r\n\
                             {}",
                            success_html.len(),
                            success_html
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        return true; // Shut down after receiving code
                    }
                }
            }

            // Implicit flow fallback: serve HTML that reads fragment tokens
            let html = CALLBACK_HTML.replace("OAUTH_PORT", &OAUTH_CALLBACK_PORT.to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n\
                 {}",
                html.len(),
                html
            );
            let _ = stream.write_all(response.as_bytes()).await;
            false
        }
        ("POST", "/auth/tokens") => {
            // Find the body after the \r\n\r\n separator
            let request_str = request.to_string();
            let body = match request_str.find("\r\n\r\n") {
                Some(idx) => &request_str[idx + 4..],
                None => {
                    send_response(&mut stream, 400, "Missing body").await;
                    return false;
                }
            };

            let tokens: AuthTokens = match serde_json::from_str(body) {
                Ok(t) => t,
                Err(e) => {
                    log::error!("[AuthServer] Failed to parse tokens: {}", e);
                    send_response(&mut stream, 400, "Invalid JSON").await;
                    return false;
                }
            };

            log::info!("[AuthServer] Received auth tokens, storing and emitting event");

            // Store tokens in global state so frontend can poll if event is missed
            if let Ok(mut guard) = PENDING_AUTH_TOKENS.lock() {
                *guard = Some(tokens.clone());
            }

            // Emit event to the frontend
            if let Err(e) = app.emit("auth-tokens-received", tokens.clone()) {
                log::error!("[AuthServer] Failed to emit auth-tokens-received: {}", e);
                send_response(&mut stream, 500, "Internal error").await;
                return false;
            }

            // Bring app window to front
            focus_main_window(&app);

            send_response(&mut stream, 200, r#"{"ok":true}"#).await;
            true // Signal to shut down
        }
        ("OPTIONS", "/auth/tokens") => {
            let response = "HTTP/1.1 204 No Content\r\n\
                            Access-Control-Allow-Origin: *\r\n\
                            Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                            Access-Control-Allow-Headers: Content-Type\r\n\
                            Content-Length: 0\r\n\
                            Connection: close\r\n\
                            \r\n";
            let _ = stream.write_all(response.as_bytes()).await;
            false
        }
        _ => {
            send_response(&mut stream, 404, "Not Found").await;
            false
        }
    }
}

async fn send_response(stream: &mut tokio::net::TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: application/json\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        status,
        reason,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

/// Bring the main window to front after OAuth completes
fn focus_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
        let _ = window.show();
        log::info!("[AuthServer] Brought main window to front after OAuth");
    }
}

/// Get and clear the pending auth code (for frontend polling fallback).
/// Returns the code if one was received before the frontend listener was ready.
#[tauri::command]
pub fn get_pending_auth_code() -> Option<String> {
    PENDING_AUTH_CODE
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

/// Get and clear the pending auth tokens (for frontend polling fallback).
/// Returns the tokens if they were received before the frontend listener was ready.
#[tauri::command]
pub fn get_pending_auth_tokens() -> Option<AuthTokens> {
    PENDING_AUTH_TOKENS
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}
