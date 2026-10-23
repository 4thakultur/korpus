use std::process::ExitCode;
use std::sync::Arc;

use korpus::config::Config;
use korpus::providers::Providers;
use korpus::web::{AppState, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // `korpus healthcheck` lets container runtimes probe readiness without
    // shipping curl in the image.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("KORPUS_LOG")
                .unwrap_or_else(|_| EnvFilter::new("korpus=info,tower_http=warn")),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let db = korpus::db::connect(&config.database_url)
        .await
        .map_err(|e| format!("could not open database: {e}"))?;
    let providers = Providers::from_config(&config);
    let names: Vec<&str> = providers.all().iter().map(|p| p.name()).collect();
    tracing::info!(providers = ?names, "food providers enabled");

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("could not bind {}: {e}", config.bind))?;
    tracing::info!("Korpus listening on http://{}", config.bind);

    let app = router(AppState {
        db: db.clone(),
        providers: Arc::new(providers),
        config: Arc::new(config),
    });
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))?;
    db.close().await;
    tracing::info!("shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "could not listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

fn healthcheck() -> ExitCode {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let Ok(config) = Config::from_env() else {
        return ExitCode::FAILURE;
    };
    let mut addr = config.bind;
    if addr.ip().is_unspecified() {
        let loopback = if addr.is_ipv4() {
            std::net::Ipv4Addr::LOCALHOST.into()
        } else {
            std::net::Ipv6Addr::LOCALHOST.into()
        };
        addr = SocketAddr::new(loopback, addr.port());
    }
    let check = || -> std::io::Result<bool> {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        write!(
            stream,
            "GET /readyz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )?;
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf)?;
        Ok(buf[..n].starts_with(b"HTTP/1.1 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}
