#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::panic;

/// Initialize Sentry crash reporting
/// Returns a guard that must be kept alive for the duration of the program
fn init_sentry() -> Option<sentry::ClientInitGuard> {
    // Sentry DSN - embedded at compile time via SENTRY_DSN env var
    // Set SENTRY_DSN in CI/CD (GitHub Secrets) to enable crash reporting in production
    let dsn = option_env!("SENTRY_DSN").unwrap_or_default().to_string();

    if dsn.is_empty() {
        tracing::info!("Sentry DSN not configured, crash reporting disabled");
        return None;
    }

    tracing::info!("Initializing Sentry crash reporting...");

    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(std::borrow::Cow::Borrowed(env!("CARGO_PKG_VERSION"))),
            environment: Some(std::borrow::Cow::Borrowed(if cfg!(debug_assertions) {
                "development"
            } else {
                "production"
            })),
            // Sample rate for error events (1.0 = 100%)
            sample_rate: 1.0,
            // Attach stacktraces to all messages
            attach_stacktrace: true,
            // Send default PII (careful with privacy)
            send_default_pii: false,
            ..Default::default()
        },
    ));

    tracing::info!("Sentry initialized successfully");
    Some(guard)
}

/// Set up custom panic hook for crash reporting
fn setup_panic_hook() {
    let original_hook = panic::take_hook();

    panic::set_hook(Box::new(move |panic_info| {
        // Log the panic
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        tracing::error!("PANIC at {}: {}", location, message);

        // Report to Sentry if configured
        sentry::capture_message(
            &format!("PANIC at {}: {}", location, message),
            sentry::Level::Fatal,
        );

        // Flush Sentry events before crashing
        // Use Hub::current().client() to access the flush functionality
        if let Some(client) = sentry::Hub::current().client() {
            client.flush(Some(std::time::Duration::from_secs(2)));
        }

        // Call the original panic hook
        original_hook(panic_info);
    }));
}

fn main() {
    // Initialize file logging with rotation (writes to both console and file)
    if let Err(e) = app_lib::logging::init_file_logging("Maity") {
        // Fallback to basic console output if file logging fails
        eprintln!(
            "Warning: Failed to initialize file logging: {}. Using console only.",
            e
        );
        // Initialize basic tracing for console
        tracing_subscriber::fmt::init();
    }

    tracing::info!("Starting Maity Desktop v{}...", env!("CARGO_PKG_VERSION"));

    // Initialize Sentry crash reporting (keep guard alive for entire program)
    let _sentry_guard = init_sentry();

    // Set up panic hook for crash reporting
    setup_panic_hook();

    // Log startup breadcrumb
    sentry::add_breadcrumb(sentry::Breadcrumb {
        category: Some("app".into()),
        message: Some("Application starting".into()),
        level: sentry::Level::Info,
        ..Default::default()
    });

    // Run the Tauri application
    app_lib::run();

    // Log clean shutdown
    tracing::info!("Application shutting down cleanly");
    sentry::add_breadcrumb(sentry::Breadcrumb {
        category: Some("app".into()),
        message: Some("Application shutdown".into()),
        level: sentry::Level::Info,
        ..Default::default()
    });
}
