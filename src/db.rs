//! Database connection and migrations.

use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens (creating if needed) the database and applies pending migrations.
/// Migrations only ever add to the schema; startup never deletes data. Search
/// text is rebuilt when it was built with an older normalization.
pub async fn connect(url: &str) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    if let Some(parent) = opts.get_filename().parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
        check_writable(parent).map_err(sqlx::Error::Io)?;
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    crate::foods::reindex_if_needed(&pool).await?;
    Ok(pool)
}

/// SQLite only reports "unable to open database file" when it lacks write
/// access. WAL mode creates files next to the database, so the directory
/// itself must be writable, not only the database file.
fn check_writable(dir: &std::path::Path) -> std::io::Result<()> {
    let probe = dir.join(".korpus-write-check");
    match std::fs::File::create(&probe) {
        Ok(_) => std::fs::remove_file(&probe),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(std::io::Error::new(
            e.kind(),
            format!(
                "Korpus cannot write to {}. Make that directory belong to the user \
                 Korpus runs as (in Docker: chown it to PUID:PGID from .env, default 1000:1000)",
                dir.display()
            ),
        )),
        Err(e) => Err(e),
    }
}

/// A private in-memory database with migrations applied, for tests.
pub async fn connect_memory() -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    // One connection: every in-memory connection would otherwise be its own database.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    crate::foods::reindex_if_needed(&pool).await?;
    Ok(pool)
}

pub fn now_utc() -> String {
    jiff::Timestamp::now()
        .strftime("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}
