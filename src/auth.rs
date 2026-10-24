//! Login: one account per installation, its password hashed with Argon2id,
//! optional time-based one-time passwords (RFC 6238) from an authenticator
//! app, and server-side sessions.
//!
//! Session tokens are random 256-bit values kept in an `HttpOnly` cookie;
//! the database holds only their SHA-256 hash. HTTP handling (cookies,
//! redirects, attempt limits) lives in `web::auth`.

use std::sync::OnceLock;

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

pub const MIN_PASSWORD_CHARS: usize = 10;
/// Upper bound so a request cannot make hashing arbitrarily expensive.
pub const MAX_PASSWORD_BYTES: usize = 1024;
pub const MAX_USERNAME_CHARS: usize = 64;
/// A session lasts this long after it was last extended.
pub const SESSION_SECS: i64 = 30 * 24 * 3600;
/// Sessions in use are extended at most this often, to avoid a database
/// write on every request.
const SESSION_REFRESH_SECS: i64 = 24 * 3600;
/// Time allowed between the password and the authenticator code.
pub const PENDING_SECS: i64 = 5 * 60;
const TOTP_PERIOD: i64 = 30;
const TOTP_DIGITS: u32 = 6;
const TOTP_SECRET_BYTES: usize = 20;

pub struct Account {
    pub username: String,
    pub totp_enabled: bool,
}

pub async fn account(db: &SqlitePool) -> AppResult<Option<Account>> {
    let row: Option<(String, bool)> =
        sqlx::query_as("SELECT username, totp_secret IS NOT NULL FROM account WHERE id = 1")
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(username, totp_enabled)| Account {
        username,
        totp_enabled,
    }))
}

pub async fn has_account(db: &SqlitePool) -> AppResult<bool> {
    Ok(sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM account")
        .fetch_one(db)
        .await?
        > 0)
}

fn random<const N: usize>() -> AppResult<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::Internal(format!("random number generator: {e}")))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares secrets without stopping at the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// Password hashes computed at once. Each takes 19 MiB and tens of
/// milliseconds, and login requests need no session, so without a bound a
/// burst of them could exhaust the server's memory; extra requests wait.
static HASHING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

async fn hashing_permit() -> AppResult<tokio::sync::SemaphorePermit<'static>> {
    HASHING
        .acquire()
        .await
        .map_err(|e| AppError::Internal(format!("password queue: {e}")))
}

/// Argon2id with the crate's defaults (19 MiB, 2 passes), which follow the
/// OWASP recommendation. Runs on a blocking thread: it takes tens of
/// milliseconds on purpose.
pub async fn hash_password(password: &str) -> AppResult<String> {
    let password = password.to_owned();
    let permit = hashing_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
    })
    .await
    .map_err(|e| AppError::Internal(format!("password task: {e}")))?
    .map_err(|e| AppError::Internal(format!("hashing password: {e}")))
}

async fn verify_password(hash: String, password: &str) -> AppResult<bool> {
    let password = password.to_owned();
    let permit = hashing_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .verify_password(password.as_bytes(), hash.as_str())
            .is_ok()
    })
    .await
    .map_err(|e| AppError::Internal(format!("password task: {e}")))
}

/// A hash checked when the username is wrong, so a wrong name takes as long
/// as a wrong password and does not reveal the account's name.
async fn dummy_hash() -> AppResult<String> {
    static DUMMY: OnceLock<String> = OnceLock::new();
    if let Some(h) = DUMMY.get() {
        return Ok(h.clone());
    }
    let h = hash_password("korpus-dummy-password").await?;
    Ok(DUMMY.get_or_init(|| h).clone())
}

fn same_username(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Creates the account unless one exists. Returns whether it was created.
pub async fn create_account(db: &SqlitePool, username: &str, password: &str) -> AppResult<bool> {
    let hash = hash_password(password).await?;
    let res = sqlx::query(
        "INSERT INTO account (id, username, password_hash) VALUES (1, ?, ?)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(username.trim())
    .bind(hash)
    .execute(db)
    .await?;
    Ok(res.rows_affected() == 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Login {
    /// Username and password are right; no second factor is set up.
    Ok,
    /// Username and password are right; the authenticator code is needed.
    NeedsCode,
    Failed,
}

pub async fn check_login(db: &SqlitePool, username: &str, password: &str) -> AppResult<Login> {
    let row: Option<(String, String, bool)> = sqlx::query_as(
        "SELECT username, password_hash, totp_secret IS NOT NULL FROM account WHERE id = 1",
    )
    .fetch_optional(db)
    .await?;
    match row {
        Some((name, hash, totp)) if same_username(&name, username) => {
            if !verify_password(hash, password).await? {
                Ok(Login::Failed)
            } else if totp {
                Ok(Login::NeedsCode)
            } else {
                Ok(Login::Ok)
            }
        }
        _ => {
            verify_password(dummy_hash().await?, password).await?;
            Ok(Login::Failed)
        }
    }
}

/// Checks the account's password, e.g. before a sensitive change.
pub async fn check_password(db: &SqlitePool, password: &str) -> AppResult<bool> {
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM account WHERE id = 1")
        .fetch_optional(db)
        .await?;
    match hash {
        Some(h) => verify_password(h, password).await,
        None => Ok(false),
    }
}

/// Sets a new password and ends every session but `keep` (the token of the
/// session making the change).
pub async fn change_password(db: &SqlitePool, password: &str, keep: &str) -> AppResult<()> {
    let hash = hash_password(password).await?;
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE account SET password_hash = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = 1",
    )
    .bind(hash)
    .execute(&mut *tx)
    .await?;
    end_other_sessions(&mut tx, keep).await?;
    tx.commit().await?;
    Ok(())
}

async fn end_other_sessions(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    keep: &str,
) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash <> ?")
        .bind(token_hash(keep))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Removes the login (account, two-factor secret and every session) and
/// keeps all diary data. The next visitor is asked to create a login with a
/// new setup code. Used by `korpus account reset`.
pub async fn reset(db: &SqlitePool) -> AppResult<bool> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM sessions")
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("DELETE FROM account").execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(res.rows_affected() > 0)
}

// --- Sessions ---------------------------------------------------------------

fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

/// Whether a cookie value can be a token at all (64 hex characters).
pub fn is_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub mfa_pending: bool,
    pub expires_at: i64,
}

/// Starts a session and returns its token. A pending session (password
/// checked, code still missing) lasts [`PENDING_SECS`].
pub async fn create_session(db: &SqlitePool, mfa_pending: bool) -> AppResult<String> {
    let token = hex(&random::<32>()?);
    let now = now();
    let lifetime = if mfa_pending {
        PENDING_SECS
    } else {
        SESSION_SECS
    };
    sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(now)
        .execute(db)
        .await?;
    sqlx::query(
        "INSERT INTO sessions (token_hash, mfa_pending, created_at, expires_at) VALUES (?, ?, ?, ?)",
    )
    .bind(token_hash(&token))
    .bind(mfa_pending)
    .bind(now)
    .bind(now + lifetime)
    .execute(db)
    .await?;
    Ok(token)
}

pub async fn find_session(db: &SqlitePool, token: &str) -> AppResult<Option<Session>> {
    if !is_token(token) {
        return Ok(None);
    }
    let row: Option<(bool, i64)> = sqlx::query_as(
        "SELECT mfa_pending, expires_at FROM sessions WHERE token_hash = ? AND expires_at > ?",
    )
    .bind(token_hash(token))
    .bind(now())
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(mfa_pending, expires_at)| Session {
        mfa_pending,
        expires_at,
    }))
}

/// Extends a session in use, at most once a day. Returns whether it was
/// extended (the cookie should then be sent again).
pub async fn refresh_session(db: &SqlitePool, token: &str, s: &Session) -> AppResult<bool> {
    let now = now();
    if s.mfa_pending || s.expires_at - now > SESSION_SECS - SESSION_REFRESH_SECS {
        return Ok(false);
    }
    sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
        .bind(now + SESSION_SECS)
        .bind(token_hash(token))
        .execute(db)
        .await?;
    Ok(true)
}

pub async fn end_session(db: &SqlitePool, token: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(token_hash(token))
        .execute(db)
        .await?;
    Ok(())
}

// --- Authenticator codes (TOTP, RFC 6238) -------------------------------------

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 without padding, as authenticator apps expect.
pub fn base32_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| *c != b'=' && *c != b' ') {
        let v = BASE32.iter().position(|x| *x == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// The code for one time step (HOTP of the step counter, RFC 4226).
pub fn totp_code(secret: &[u8], step: i64) -> u32 {
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(secret) else {
        // HMAC accepts keys of any length.
        return u32::MAX;
    };
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[digest.len() - 1] & 0x0f);
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 10u32.pow(TOTP_DIGITS)
}

/// The six digits of a code as typed ("123 456" is accepted).
fn parse_code(code: &str) -> Option<String> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    (digits.len() == TOTP_DIGITS as usize && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(digits)
}

/// The time step `code` belongs to, allowing one step of clock difference
/// either way, and only if it is later than `after` (a code used before).
pub fn match_code(secret_b32: &str, code: &str, unix: i64, after: Option<i64>) -> Option<i64> {
    let code = parse_code(code)?;
    let secret = base32_decode(secret_b32)?;
    let current = unix.div_euclid(TOTP_PERIOD);
    let mut found = None;
    // Every candidate is computed and compared, so timing does not reveal
    // which one matched.
    for step in [current - 1, current, current + 1] {
        let expected = format!(
            "{:0width$}",
            totp_code(&secret, step),
            width = TOTP_DIGITS as usize
        );
        if constant_time_eq(expected.as_bytes(), code.as_bytes()) && after.is_none_or(|a| step > a)
        {
            found = Some(step);
        }
    }
    found
}

/// `otpauth://` link encoded in the QR code for authenticator apps.
pub fn otpauth_uri(secret_b32: &str, username: &str) -> String {
    let label = crate::web::url_encode(&format!("Korpus:{username}")).replace('+', "%20");
    format!(
        "otpauth://totp/{label}?secret={secret_b32}&issuer=Korpus&algorithm=SHA1&digits={TOTP_DIGITS}&period={TOTP_PERIOD}"
    )
}

/// Starts setting up two-factor login with a new secret, replacing any
/// earlier one that was never confirmed. A fresh secret each time means a
/// secret seen by someone else (say, on a borrowed session) is useless once
/// the owner starts again.
pub async fn new_pending_totp_secret(db: &SqlitePool) -> AppResult<String> {
    let secret = base32_encode(&random::<TOTP_SECRET_BYTES>()?);
    let res = sqlx::query(
        "UPDATE account SET totp_pending_secret = ? WHERE id = 1 AND totp_secret IS NULL",
    )
    .bind(&secret)
    .execute(db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(secret)
}

/// Turns two-factor login on if `code` matches the secret being set up,
/// and ends every other session (they were opened with the password only).
pub async fn enable_totp(db: &SqlitePool, code: &str, keep: &str) -> AppResult<bool> {
    let pending: Option<String> =
        sqlx::query_scalar("SELECT totp_pending_secret FROM account WHERE id = 1")
            .fetch_optional(db)
            .await?
            .flatten();
    let Some(secret) = pending else {
        return Ok(false);
    };
    let Some(step) = match_code(&secret, code, now(), None) else {
        return Ok(false);
    };
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE account SET totp_secret = totp_pending_secret, totp_pending_secret = NULL,
             totp_last_step = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1",
    )
    .bind(step)
    .execute(&mut *tx)
    .await?;
    end_other_sessions(&mut tx, keep).await?;
    tx.commit().await?;
    Ok(true)
}

/// Checks a login code. Each code is accepted once: the step is recorded
/// atomically, so a code seen by someone else cannot be replayed.
pub async fn verify_totp(db: &SqlitePool, code: &str) -> AppResult<bool> {
    let row: Option<(Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT totp_secret, totp_last_step FROM account WHERE id = 1")
            .fetch_optional(db)
            .await?;
    let Some((Some(secret), last)) = row else {
        return Ok(false);
    };
    let Some(step) = match_code(&secret, code, now(), last) else {
        return Ok(false);
    };
    let res = sqlx::query(
        "UPDATE account SET totp_last_step = ?
         WHERE id = 1 AND (totp_last_step IS NULL OR totp_last_step < ?)",
    )
    .bind(step)
    .bind(step)
    .execute(db)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn disable_totp(db: &SqlitePool) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE account SET totp_secret = NULL, totp_pending_secret = NULL,
             totp_last_step = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE id = 1 AND totp_secret IS NOT NULL",
    )
    .execute(db)
    .await?;
    Ok(res.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B (SHA-1), truncated to six digits.
    #[test]
    fn totp_matches_rfc_6238_vectors() {
        let secret = b"12345678901234567890";
        for (t, code) in [
            (59, 287_082),
            (1_111_111_109, 81_804),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
        ] {
            assert_eq!(totp_code(secret, t / 30), code, "t={t}");
        }
        let b32 = base32_encode(secret);
        assert_eq!(b32, "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(match_code(&b32, "287082", 59, None), Some(1));
        assert_eq!(
            match_code(&b32, "287 082", 59 + 30, None),
            Some(1),
            "one step late"
        );
        assert_eq!(match_code(&b32, "287082", 59 + 90, None), None, "too late");
        assert_eq!(
            match_code(&b32, "287082", 59, Some(1)),
            None,
            "already used"
        );
        assert_eq!(match_code(&b32, "28708", 59, None), None);
        assert_eq!(match_code(&b32, "abcdef", 59, None), None);
    }

    #[test]
    fn base32_round_trips() {
        for len in 0..25 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base32_decode(&base32_encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(base32_decode("not base32!"), None);
    }

    #[test]
    fn otpauth_uri_encodes_the_label() {
        let uri = otpauth_uri("ABC", "me & you");
        assert!(uri.starts_with("otpauth://totp/Korpus%3Ame%20%26%20you?secret=ABC&issuer=Korpus"));
    }

    #[tokio::test]
    async fn login_sessions_and_second_factor() {
        let db = crate::db::connect_memory().await.unwrap();
        assert!(!has_account(&db).await.unwrap());
        assert!(
            create_account(&db, " Me ", "correct horse battery")
                .await
                .unwrap()
        );
        assert!(
            !create_account(&db, "other", "another password")
                .await
                .unwrap()
        );
        let stored: String = sqlx::query_scalar("SELECT password_hash FROM account")
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(
            stored.starts_with("$argon2id$"),
            "never stored in plain text"
        );

        assert_eq!(
            check_login(&db, "me", "correct horse battery")
                .await
                .unwrap(),
            Login::Ok
        );
        assert_eq!(
            check_login(&db, "me", "wrong password").await.unwrap(),
            Login::Failed
        );
        assert_eq!(
            check_login(&db, "you", "correct horse battery")
                .await
                .unwrap(),
            Login::Failed
        );

        let token = create_session(&db, false).await.unwrap();
        assert!(is_token(&token));
        let session = find_session(&db, &token).await.unwrap().unwrap();
        assert!(!session.mfa_pending);
        assert!(
            !refresh_session(&db, &token, &session).await.unwrap(),
            "fresh"
        );
        let hashes: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_all(&db)
            .await
            .unwrap();
        assert!(!hashes.contains(&token), "only the hash is stored");

        let other = create_session(&db, false).await.unwrap();
        change_password(&db, "new password 123", &token)
            .await
            .unwrap();
        assert!(
            find_session(&db, &other).await.unwrap().is_none(),
            "others end"
        );
        assert!(
            find_session(&db, &token).await.unwrap().is_some(),
            "own stays"
        );
        assert!(check_password(&db, "new password 123").await.unwrap());

        // Two-factor: starting again replaces an unconfirmed secret.
        let abandoned = new_pending_totp_secret(&db).await.unwrap();
        let secret = new_pending_totp_secret(&db).await.unwrap();
        assert_ne!(abandoned, secret);
        let code = |s: &str| {
            format!(
                "{:06}",
                totp_code(&base32_decode(s).unwrap(), now() / TOTP_PERIOD)
            )
        };
        let wrong = (0..4)
            .map(|i| format!("{i}{i}{i}{i}{i}{i}"))
            .find(|c| match_code(&secret, c, now(), None).is_none())
            .unwrap();
        assert!(!enable_totp(&db, &wrong, &token).await.unwrap());
        assert!(enable_totp(&db, &code(&secret), &token).await.unwrap());
        assert!(account(&db).await.unwrap().unwrap().totp_enabled);
        assert!(
            new_pending_totp_secret(&db).await.is_err(),
            "cannot replace the confirmed secret"
        );
        assert_eq!(
            check_login(&db, "ME", "new password 123").await.unwrap(),
            Login::NeedsCode
        );
        // The code that confirmed set-up cannot be used again to log in.
        assert!(!verify_totp(&db, &code(&secret)).await.unwrap());

        assert!(disable_totp(&db).await.unwrap());
        assert!(reset(&db).await.unwrap());
        assert!(!has_account(&db).await.unwrap());
        assert!(find_session(&db, &token).await.unwrap().is_none());
    }
}
