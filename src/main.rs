use std::process::ExitCode;
use std::sync::Arc;

use korpus::config::Config;
use korpus::providers::Providers;
use korpus::web::{AppState, Guard, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    // `korpus healthcheck` lets container runtimes probe readiness without
    // shipping curl in the image.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("healthcheck") {
        return healthcheck();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("KORPUS_LOG")
                .unwrap_or_else(|_| EnvFilter::new("korpus=info,tower_http=warn")),
        )
        .init();

    if args.first().map(String::as_str) == Some("backup") {
        return match backup_command(&args[1..]).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if args.first().map(String::as_str) == Some("account") {
        return match account_command(&args[1..]).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

    if args.first().map(String::as_str) == Some("dataset") {
        return match dataset_command(&args[1..]).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }

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
    // A bundled table that cannot be installed must not keep the diary
    // from starting; search then works without it.
    if let Err(e) = korpus::datasets::install_bundled(&db).await {
        tracing::error!(error = %e, "could not install bundled food tables");
    }
    let providers = Providers::from_config(&config);
    let names: Vec<&str> = providers.all().iter().map(|p| p.name()).collect();
    tracing::info!(providers = ?names, "food providers enabled");

    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|e| format!("could not bind {}: {e}", config.bind))?;
    tracing::info!("Korpus listening on http://{}", config.bind);

    let auth = Arc::new(Guard::default());
    match korpus::auth::has_account(&db).await {
        // Logged right away so the code is in the log before anyone asks.
        Ok(false) => {
            if let Err(e) = auth.setup_code() {
                tracing::error!(error = %e, "could not create a setup code");
            }
        }
        Ok(true) => {}
        Err(e) => tracing::error!(error = %e, "could not read the login account"),
    }

    let app = router(AppState {
        db: db.clone(),
        providers: Arc::new(providers),
        config: Arc::new(config),
        auth,
    });
    // Client addresses limit failed login attempts per client.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(|e| format!("server error: {e}"))?;
    db.close().await;
    tracing::info!("shut down cleanly");
    Ok(())
}

/// `korpus backup <file>`: a consistent copy of the database for scripts
/// (the web download needs a login). Safe while the server runs.
async fn backup_command(args: &[String]) -> Result<(), String> {
    let [path] = args else {
        return Err("usage: korpus backup <file>   (the file must not exist yet)".into());
    };
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let db = korpus::db::connect(&config.database_url)
        .await
        .map_err(|e| format!("could not open database: {e}"))?;
    let result = korpus::backup::write_snapshot(&db, std::path::Path::new(path))
        .await
        .map(|()| println!("Backup written to {path}."))
        .map_err(|e| format!("backup failed: {e}"));
    db.close().await;
    result
}

const ACCOUNT_USAGE: &str = "usage:
  korpus account reset         remove the login (diary data is kept); the next visitor
                               creates a new one with a setup code from the server log
  korpus account disable-2fa   turn off two-factor login, e.g. after losing the phone";

/// `korpus account …`: recovery when the password or authenticator is lost.
/// Needs access to the server (and its database), not to the web page.
async fn account_command(args: &[String]) -> Result<(), String> {
    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let db = korpus::db::connect(&config.database_url)
        .await
        .map_err(|e| format!("could not open database: {e}"))?;
    let result = match args.first().map(String::as_str) {
        Some("reset") => korpus::auth::reset(&db)
            .await
            .map(|removed| {
                if removed {
                    println!(
                        "Login removed and every device logged out. Restart Korpus (or open it) to get a setup code for a new login."
                    );
                } else {
                    println!("No login exists yet.");
                }
            })
            .map_err(|e| e.to_string()),
        Some("disable-2fa") => korpus::auth::disable_totp(&db)
            .await
            .map(|changed| {
                if changed {
                    println!("Two-factor login is off. Log in with the username and password.");
                } else {
                    println!("Two-factor login was not on.");
                }
            })
            .map_err(|e| e.to_string()),
        _ => Err(ACCOUNT_USAGE.to_string()),
    };
    db.close().await;
    result
}

const DATASET_USAGE: &str = "usage:
  korpus dataset list
  korpus dataset import <usda|ciqual|cofid|afcd|cnf> <file>...
  korpus dataset remove <usda|ciqual|cofid|afcd|cnf>
  korpus dataset install-bundled ciqual
  korpus dataset extract ciqual <alim_….xml> <compo_….xml>   (writes the bundled extract to stdout)";

/// `korpus dataset …`: manages food composition tables from the command
/// line, e.g. for scripted installs. Settings offers the same in the browser.
async fn dataset_command(args: &[String]) -> Result<(), String> {
    use korpus::datasets::{self, Bundled, Dataset, UploadFile};

    let read_files = |paths: &[String]| -> Result<Vec<UploadFile>, String> {
        let mut files = Vec::new();
        for path in paths {
            let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
            if bytes.len() > datasets::MAX_UPLOAD_BYTES {
                return Err(format!("{path}: file is too large"));
            }
            files.push(UploadFile {
                name: path.clone(),
                bytes,
            });
        }
        Ok(files)
    };
    // Needs no database: it prepares the file shipped in `bundled/`.
    if args.first().map(String::as_str) == Some("extract") {
        if args.get(1).map(String::as_str) != Some("ciqual") {
            return Err(DATASET_USAGE.to_string());
        }
        let files = read_files(&args[2..])?;
        let text = datasets::ciqual_extract(&files).map_err(|e| {
            format!(
                "{} ({e})",
                e.message(Dataset::Ciqual, korpus::i18n::Lang::En)
            )
        })?;
        print!("{text}");
        return Ok(());
    }

    let config = Config::from_env().map_err(|e| format!("configuration error: {e}"))?;
    let db = korpus::db::connect(&config.database_url)
        .await
        .map_err(|e| format!("could not open database: {e}"))?;
    let dataset = |id: Option<&String>| {
        id.and_then(|id| Dataset::parse(id))
            .ok_or_else(|| DATASET_USAGE.to_string())
    };
    let lang = korpus::i18n::Lang::En;
    let result = match args.first().map(String::as_str) {
        Some("list") => {
            let installed = datasets::installed(&db).await.map_err(|e| e.to_string())?;
            for d in Dataset::ALL {
                let status = match installed.iter().find(|i| i.dataset == d) {
                    Some(i) => match (&i.bundled_version, Bundled::get(d)) {
                        (Some(_), Some(b)) => format!(
                            "{} foods, {} included with Korpus, installed {}",
                            i.food_count, b.label, i.imported_at
                        ),
                        _ => format!("{} foods, imported {}", i.food_count, i.imported_at),
                    },
                    None => format!("not imported; download: {}", d.download_url()),
                };
                println!(
                    "{:<7} {} ({}): {status}",
                    d.id(),
                    d.name(),
                    d.country_name(lang)
                );
            }
            Ok(())
        }
        Some("import") => {
            let d = dataset(args.get(1))?;
            let files = read_files(&args[2..])?;
            let foods = tokio::task::spawn_blocking(move || d.parse_files(&files))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("{} ({e})", e.message(d, lang)))?;
            let stored = datasets::replace(&db, d, &foods, None)
                .await
                .map_err(|e| e.to_string())?;
            println!("Imported {stored} foods from {}.", d.name());
            Ok(())
        }
        Some("install-bundled") => {
            let d = dataset(args.get(1))?;
            match Bundled::get(d) {
                Some(b) => {
                    let stored = b.install(&db).await.map_err(|e| e.to_string())?;
                    println!("Installed {stored} foods from {}.", b.label);
                    Ok(())
                }
                None => Err(format!("{} is not included with Korpus.", d.name())),
            }
        }
        Some("remove") => {
            let d = dataset(args.get(1))?;
            if datasets::remove(&db, d).await.map_err(|e| e.to_string())? {
                println!(
                    "Removed {}. Foods already saved to your foods are kept.",
                    d.name()
                );
            } else {
                println!("{} was not imported.", d.name());
            }
            Ok(())
        }
        _ => Err(DATASET_USAGE.to_string()),
    };
    db.close().await;
    result
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
