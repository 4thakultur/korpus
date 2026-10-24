//! Login pages, the session cookie and the middleware that requires a
//! login for everything but the login pages, static assets and health
//! checks.
//!
//! The first visitor creates the account with a setup code printed in the
//! server log, so whoever reaches a fresh (or freshly upgraded) installation
//! first cannot claim it. Failed attempts are limited per client address.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use askama::Template;
use axum::Form;
use axum::extract::{ConnectInfo, FromRequestParts, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{AppState, current_lang, render_status, see_other, url_encode, wants_fragment};
use crate::auth::{self, Login};
use crate::config::IpRange;
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::i18n::Lang;

pub const COOKIE: &str = "korpus_session";
/// Login attempts (password, code or setup code) counted per client. An
/// attempt counts until it succeeds.
const ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);
const MAX_FAILURES_PER_CLIENT: usize = 10;
/// Across all clients, so many addresses cannot guess in parallel.
const MAX_FAILURES_TOTAL: usize = 100;
/// Unambiguous characters for the setup code (no 0/O, 1/I).
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// Login state kept in memory: recent failures and the setup code.
#[derive(Default)]
pub struct Guard {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
    setup_code: Mutex<Option<String>>,
}

impl Guard {
    /// Counts an attempt by `client`, or returns false if it must wait.
    ///
    /// Checking and counting happen under one lock, before the attempt is
    /// judged: checking first and counting failures afterwards would let
    /// many requests sent at once all pass the check.
    pub fn attempt(&self, client: &str) -> bool {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        map.retain(|_, times| {
            times.retain(|t| t.elapsed() < ATTEMPT_WINDOW);
            !times.is_empty()
        });
        let total: usize = map.values().map(Vec::len).sum();
        let times = map.entry(client.to_string()).or_default();
        if total >= MAX_FAILURES_TOTAL || times.len() >= MAX_FAILURES_PER_CLIENT {
            return false;
        }
        times.push(Instant::now());
        true
    }

    pub fn succeeded(&self, client: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(client);
    }

    /// The code needed to create the account, generated (and written to the
    /// log) the first time it is needed.
    pub fn setup_code(&self) -> AppResult<String> {
        let mut code = self.setup_code.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = code.as_ref() {
            return Ok(c.clone());
        }
        let mut bytes = [0u8; 10];
        getrandom::fill(&mut bytes)
            .map_err(|e| AppError::Internal(format!("random number generator: {e}")))?;
        let chars: String = bytes
            .iter()
            .map(|b| CODE_ALPHABET[usize::from(b % 32)] as char)
            .collect();
        let c = format!("{}-{}", &chars[..5], &chars[5..]);
        // Deliberately logged: reading the server log proves the visitor
        // runs this server. It is useless once the account exists.
        tracing::warn!(
            "No login exists yet. Open Korpus in your browser and enter this setup code to create it: {c}"
        );
        *code = Some(c.clone());
        Ok(c)
    }

    fn setup_code_matches(&self, input: &str) -> bool {
        let normalize = |s: &str| -> String {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .collect()
        };
        let code = self.setup_code.lock().unwrap_or_else(|p| p.into_inner());
        code.as_deref().is_some_and(|c| {
            auth::constant_time_eq(normalize(c).as_bytes(), normalize(input).as_bytes())
        })
    }

    fn clear_setup_code(&self) {
        *self.setup_code.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// The client address used to count failed attempts. Behind a reverse
/// proxy every connection comes from the proxy, so its `X-Forwarded-For` is
/// used when the proxy is listed in `KORPUS_TRUSTED_PROXIES`; otherwise the
/// limit applies to all clients together.
pub struct Client(String);

impl FromRequestParts<AppState> for Client {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Ok(Client(match peer {
            Some(peer) => client_key(client_ip(
                peer,
                &parts.headers,
                &state.config.trusted_proxies,
            )),
            None => "local".into(),
        }))
    }
}

/// The client behind any trusted proxies: `X-Forwarded-For` read from the
/// right (each proxy appends the address it saw), skipping trusted proxies.
/// Entries further left are written by the client and could be forged.
fn client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpRange]) -> IpAddr {
    let is_trusted = |ip: IpAddr| trusted.iter().any(|r| r.contains(ip));
    let mut client = peer.to_canonical();
    if !is_trusted(client) {
        return client;
    }
    let hops: Vec<&str> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .collect();
    for hop in hops.into_iter().rev() {
        let Ok(ip) = hop.trim().parse::<IpAddr>() else {
            break;
        };
        client = ip.to_canonical();
        if !is_trusted(client) {
            break;
        }
    }
    client
}

/// IPv6 clients are counted per /64, the block one home connection gets,
/// so that switching addresses within it does not reset the count.
fn client_key(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let net = std::net::Ipv6Addr::from(u128::from(v6) & !((1u128 << 64) - 1));
            format!("{net}/64")
        }
    }
}

/// The token of the logged-in session handling the request.
#[derive(Clone)]
pub struct CurrentSession(pub String);

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_string())
        .filter(|t| auth::is_token(t))
}

/// Cookies get `Secure` when the reverse proxy reports HTTPS. A forged
/// header only affects the forger's own cookie.
fn is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("https"))
}

fn cookie(headers: &HeaderMap, value: &str, max_age: i64) -> HeaderValue {
    let secure = if is_https(headers) { "; Secure" } else { "" };
    let c = format!("{COOKIE}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}");
    // Tokens are hex, so the value is always a valid header.
    HeaderValue::from_str(&c).unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn with_cookie(mut res: Response, value: HeaderValue) -> Response {
    res.headers_mut().append(header::SET_COOKIE, value);
    res
}

/// A local path to continue to after logging in; anything else (other
/// sites, `//host`, odd characters) falls back to the diary.
pub fn safe_next(next: &str) -> &str {
    let ok = next.starts_with('/')
        && !next.starts_with("//")
        && !next.starts_with("/\\")
        && next.len() <= 1024
        && next.bytes().all(|b| b.is_ascii_graphic())
        && !next.starts_with("/login")
        && !next.starts_with("/logout")
        && !next.starts_with("/account");
    if ok { next } else { "/" }
}

fn login_url(next: &str) -> String {
    match safe_next(next) {
        "/" => "/login".into(),
        n => format!("/login?next={}", url_encode(n)),
    }
}

/// Paths reachable without a login.
fn is_public(path: &str) -> bool {
    matches!(
        path,
        "/login"
            | "/login/code"
            | "/account/setup"
            | "/healthz"
            | "/readyz"
            | "/static/app.css"
            | "/static/app.js"
            | "/static/favicon.svg"
            | "/static/apple-touch-icon.png"
            | "/apple-touch-icon.png"
    )
}

/// Requires a full (not pending) session for every non-public path.
pub async fn require_login(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    if is_public(req.uri().path()) {
        return next.run(req).await;
    }
    match check(&state, &mut req).await {
        Ok(Ok(refresh)) => {
            let headers = req.headers().clone();
            let res = next.run(req).await;
            match refresh {
                Some(token) => with_cookie(res, cookie(&headers, &token, auth::SESSION_SECS)),
                None => res,
            }
        }
        Ok(Err(res)) => res,
        Err(e) => e.into_response(),
    }
}

/// `Ok(Ok(token to send again, if the session was extended))` when logged
/// in, `Ok(Err(response))` to send instead.
async fn check(state: &AppState, req: &mut Request) -> AppResult<Result<Option<String>, Response>> {
    if let Some(token) = session_token(req.headers())
        && let Some(s) = auth::find_session(&state.db, &token).await?
    {
        if s.mfa_pending {
            return Ok(Err(see_other("/login/code")));
        }
        let refreshed = auth::refresh_session(&state.db, &token, &s).await?;
        req.extensions_mut().insert(CurrentSession(token.clone()));
        return Ok(Ok(refreshed.then_some(token)));
    }
    if wants_fragment(req.headers()) {
        return Ok(Err(StatusCode::UNAUTHORIZED.into_response()));
    }
    if !auth::has_account(&state.db).await? {
        return Ok(Err(see_other("/account/setup")));
    }
    let next = match *req.method() {
        axum::http::Method::GET => req.uri().path_and_query().map_or("/", |p| p.as_str()),
        _ => "/",
    };
    Ok(Err(see_other(&login_url(next))))
}

/// Starts a full session and continues to `next`.
async fn log_in(state: &AppState, headers: &HeaderMap, next: &str) -> AppResult<Response> {
    if let Some(old) = session_token(headers) {
        auth::end_session(&state.db, &old).await?;
    }
    let token = auth::create_session(&state.db, false).await?;
    Ok(with_cookie(
        see_other(safe_next(next)),
        cookie(headers, &token, auth::SESSION_SECS),
    ))
}

fn too_many(lang: Lang) -> String {
    lang.t("Too many failed attempts. Wait a few minutes and try again.")
        .into()
}

/// Username and new-password checks shared by account creation.
fn check_new_password(
    password: &str,
    confirm: &str,
    field: &'static str,
    confirm_field: &'static str,
    errors: &mut FieldErrors,
    lang: Lang,
) {
    if password.chars().count() < auth::MIN_PASSWORD_CHARS {
        errors.add(
            field,
            lang.f(
                "The password must have at least {0} characters.",
                &[&auth::MIN_PASSWORD_CHARS],
            ),
        );
    } else if password.len() > auth::MAX_PASSWORD_BYTES {
        errors.add(field, lang.t("The password is too long."));
    } else if password != confirm {
        errors.add(confirm_field, lang.t("The passwords do not match."));
    }
}

// --- Account creation ---------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct AccountForm {
    username: String,
    password: String,
    password_confirm: String,
    setup_code: String,
}

#[derive(Template)]
#[template(path = "account_setup.html")]
pub struct AccountSetupPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub username: String,
    pub errors: FieldErrors,
    pub min_password: usize,
}

fn account_setup_page(lang: Lang, username: String, errors: FieldErrors) -> AccountSetupPage {
    AccountSetupPage {
        lang,
        title: lang.t("Create your account").into(),
        nav: "",
        username,
        errors,
        min_password: auth::MIN_PASSWORD_CHARS,
    }
}

pub async fn account_setup_form(State(state): State<AppState>) -> AppResult<Response> {
    if auth::has_account(&state.db).await? {
        return Ok(see_other("/login"));
    }
    state.auth.setup_code()?;
    render_status(
        StatusCode::OK,
        &account_setup_page(current_lang(), String::new(), FieldErrors::default()),
    )
}

pub async fn account_setup(
    State(state): State<AppState>,
    Client(client): Client,
    headers: HeaderMap,
    Form(form): Form<AccountForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    if auth::has_account(&state.db).await? {
        return Ok(see_other("/login"));
    }
    let mut errors = FieldErrors::default();
    let username = form.username.trim();
    if username.is_empty() {
        errors.add("username", lang.t("Enter a username."));
    } else if username.chars().count() > auth::MAX_USERNAME_CHARS {
        errors.add(
            "username",
            lang.f(
                "The username can have at most {0} characters.",
                &[&auth::MAX_USERNAME_CHARS],
            ),
        );
    }
    check_new_password(
        &form.password,
        &form.password_confirm,
        "password",
        "password_confirm",
        &mut errors,
        lang,
    );
    // Make sure a code exists (and is logged) even if the form was posted
    // without loading the page.
    state.auth.setup_code()?;
    if !state.auth.attempt(&client) {
        errors.add("setup_code", too_many(lang));
    } else if state.auth.setup_code_matches(&form.setup_code) {
        state.auth.succeeded(&client);
    } else {
        errors.add(
            "setup_code",
            lang.t("The setup code is incorrect. Check the server log."),
        );
    }
    if !errors.is_empty() {
        return render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &account_setup_page(lang, username.to_string(), errors),
        );
    }
    if !auth::create_account(&state.db, username, &form.password).await? {
        return Ok(see_other("/login"));
    }
    state.auth.clear_setup_code();
    tracing::info!("login account created");
    log_in(&state, &headers, "/").await
}

// --- Logging in -----------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct NextQuery {
    next: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct LoginForm {
    username: String,
    password: String,
    next: String,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub username: String,
    pub next: String,
    pub error: Option<String>,
}

fn login_page(lang: Lang, username: String, next: &str, error: Option<String>) -> LoginPage {
    LoginPage {
        lang,
        title: lang.t("Log in").into(),
        nav: "",
        username,
        next: safe_next(next).to_string(),
        error,
    }
}

async fn logged_in(state: &AppState, headers: &HeaderMap) -> AppResult<bool> {
    Ok(match session_token(headers) {
        Some(t) => auth::find_session(&state.db, &t)
            .await?
            .is_some_and(|s| !s.mfa_pending),
        None => false,
    })
}

pub async fn login_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<NextQuery>,
) -> AppResult<Response> {
    if !auth::has_account(&state.db).await? {
        return Ok(see_other("/account/setup"));
    }
    if logged_in(&state, &headers).await? {
        return Ok(see_other(safe_next(&q.next)));
    }
    render_status(
        StatusCode::OK,
        &login_page(current_lang(), String::new(), &q.next, None),
    )
}

pub async fn login(
    State(state): State<AppState>,
    Client(client): Client,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let fail = |status, msg: String| {
        render_status(
            status,
            &login_page(
                lang,
                form.username.trim().to_string(),
                &form.next,
                Some(msg),
            ),
        )
    };
    if !state.auth.attempt(&client) {
        return fail(StatusCode::TOO_MANY_REQUESTS, too_many(lang));
    }
    if form.password.len() > auth::MAX_PASSWORD_BYTES {
        return fail(
            StatusCode::UNAUTHORIZED,
            lang.t("The username or password is incorrect.").into(),
        );
    }
    match auth::check_login(&state.db, &form.username, &form.password).await? {
        Login::Failed => {
            tracing::warn!(client = %client, "failed login");
            fail(
                StatusCode::UNAUTHORIZED,
                lang.t("The username or password is incorrect.").into(),
            )
        }
        Login::Ok => {
            state.auth.succeeded(&client);
            log_in(&state, &headers, &form.next).await
        }
        // The attempt still counts until the code is right too.
        Login::NeedsCode => {
            if let Some(old) = session_token(&headers) {
                auth::end_session(&state.db, &old).await?;
            }
            let token = auth::create_session(&state.db, true).await?;
            let next = match safe_next(&form.next) {
                "/" => "/login/code".to_string(),
                n => format!("/login/code?next={}", url_encode(n)),
            };
            Ok(with_cookie(
                see_other(&next),
                cookie(&headers, &token, auth::PENDING_SECS),
            ))
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct CodeForm {
    code: String,
    next: String,
}

#[derive(Template)]
#[template(path = "login_code.html")]
pub struct CodePage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub next: String,
    pub error: Option<String>,
}

fn code_page(lang: Lang, next: &str, error: Option<String>) -> CodePage {
    CodePage {
        lang,
        title: lang.t("Two-factor authentication").into(),
        nav: "",
        next: safe_next(next).to_string(),
        error,
    }
}

/// The token of a session waiting for its authenticator code.
async fn pending_session(state: &AppState, headers: &HeaderMap) -> AppResult<Option<String>> {
    Ok(match session_token(headers) {
        Some(t) => auth::find_session(&state.db, &t)
            .await?
            .filter(|s| s.mfa_pending)
            .map(|_| t),
        None => None,
    })
}

pub async fn code_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<NextQuery>,
) -> AppResult<Response> {
    if pending_session(&state, &headers).await?.is_none() {
        return Ok(see_other(&login_url(&q.next)));
    }
    render_status(StatusCode::OK, &code_page(current_lang(), &q.next, None))
}

pub async fn code(
    State(state): State<AppState>,
    Client(client): Client,
    headers: HeaderMap,
    Form(form): Form<CodeForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let Some(pending) = pending_session(&state, &headers).await? else {
        return Ok(see_other(&login_url(&form.next)));
    };
    if !state.auth.attempt(&client) {
        return render_status(
            StatusCode::TOO_MANY_REQUESTS,
            &code_page(lang, &form.next, Some(too_many(lang))),
        );
    }
    if !auth::verify_totp(&state.db, &form.code).await? {
        tracing::warn!(client = %client, "wrong authenticator code");
        return render_status(
            StatusCode::UNAUTHORIZED,
            &code_page(
                lang,
                &form.next,
                Some(lang.t("The code is incorrect or has expired.").into()),
            ),
        );
    }
    state.auth.succeeded(&client);
    auth::end_session(&state.db, &pending).await?;
    let token = auth::create_session(&state.db, false).await?;
    Ok(with_cookie(
        see_other(safe_next(&form.next)),
        cookie(&headers, &token, auth::SESSION_SECS),
    ))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = session_token(&headers) {
        auth::end_session(&state.db, &token).await?;
    }
    Ok(with_cookie(see_other("/login"), cookie(&headers, "", 0)))
}

// --- Account settings -------------------------------------------------------------

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct PasswordForm {
    current: String,
    password: String,
    password_confirm: String,
}

#[derive(Template)]
#[template(path = "account_password.html")]
pub struct PasswordPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub errors: FieldErrors,
    pub min_password: usize,
}

fn password_page(lang: Lang, errors: FieldErrors) -> PasswordPage {
    PasswordPage {
        lang,
        title: lang.t("Change password").into(),
        nav: "settings",
        errors,
        min_password: auth::MIN_PASSWORD_CHARS,
    }
}

pub async fn password_form() -> AppResult<Response> {
    render_status(
        StatusCode::OK,
        &password_page(current_lang(), FieldErrors::default()),
    )
}

/// Checks the current password before a sensitive change, counting
/// failures like a login (a stolen session must not allow guessing).
async fn confirm_password(
    state: &AppState,
    client: &str,
    password: &str,
    field: &'static str,
    errors: &mut FieldErrors,
    lang: Lang,
) -> AppResult<()> {
    if !state.auth.attempt(client) {
        errors.add(field, too_many(lang));
    } else if password.len() > auth::MAX_PASSWORD_BYTES
        || !auth::check_password(&state.db, password).await?
    {
        errors.add(field, lang.t("The current password is incorrect."));
    } else {
        state.auth.succeeded(client);
    }
    Ok(())
}

pub async fn change_password(
    State(state): State<AppState>,
    Client(client): Client,
    axum::Extension(CurrentSession(token)): axum::Extension<CurrentSession>,
    Form(form): Form<PasswordForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let mut errors = FieldErrors::default();
    confirm_password(&state, &client, &form.current, "current", &mut errors, lang).await?;
    check_new_password(
        &form.password,
        &form.password_confirm,
        "password",
        "password_confirm",
        &mut errors,
        lang,
    );
    if !errors.is_empty() {
        return render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &password_page(lang, errors),
        );
    }
    auth::change_password(&state.db, &form.password, &token).await?;
    tracing::info!("password changed");
    Ok(see_other("/settings?msg=password_changed#account-heading"))
}

/// Where the two-factor page is: the secret is shown only right after the
/// password was entered, so a session alone (a device left logged in, a
/// copied cookie) cannot read it, or turn the setting on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TwoFactorStep {
    /// On: the form to turn it off.
    Enabled,
    /// Off: the password is asked before a secret is made.
    Start,
    /// The new secret and its QR code, and the field for the first code.
    Scan,
    /// A wrong first code: only the code field again, not the secret.
    Retry,
}

#[derive(Template)]
#[template(path = "account_2fa.html")]
pub struct TwoFactorPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub step: TwoFactorStep,
    /// While scanning: the key and its QR code.
    pub secret: String,
    pub qr_svg: String,
    pub errors: FieldErrors,
}

/// The `otpauth://` link as an inline SVG QR code (no image request, so
/// the secret never appears in a URL or cache).
fn qr_svg(uri: &str, lang: Lang) -> AppResult<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(uri.as_bytes())
        .map_err(|e| AppError::Internal(format!("QR code: {e}")))?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    let image = image
        .split_once("?>")
        .map_or(image.as_str(), |(_, svg)| svg)
        .to_string();
    let label = lang
        .t("QR code for your authenticator app")
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    Ok(image.replacen(
        "<svg ",
        &format!(r#"<svg class="qr" role="img" aria-label="{label}" "#),
        1,
    ))
}

fn two_factor_page(step: TwoFactorStep, errors: FieldErrors) -> TwoFactorPage {
    let lang = current_lang();
    TwoFactorPage {
        lang,
        title: lang.t("Two-factor authentication").into(),
        nav: "settings",
        step,
        secret: String::new(),
        qr_svg: String::new(),
        errors,
    }
}

/// The page for the current state when nothing was submitted (or the
/// submission does not fit the state).
async fn two_factor_overview(state: &AppState, errors: FieldErrors) -> AppResult<TwoFactorPage> {
    let account = auth::account(&state.db).await?.ok_or(AppError::NotFound)?;
    let step = if account.totp_enabled {
        TwoFactorStep::Enabled
    } else {
        TwoFactorStep::Start
    };
    Ok(two_factor_page(step, errors))
}

pub async fn two_factor_form(State(state): State<AppState>) -> AppResult<Response> {
    let page = two_factor_overview(&state, FieldErrors::default()).await?;
    render_status(StatusCode::OK, &page)
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct StartForm {
    current: String,
}

/// Checks the password, then shows a new secret to scan.
pub async fn start_two_factor(
    State(state): State<AppState>,
    Client(client): Client,
    Form(form): Form<StartForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let account = auth::account(&state.db).await?.ok_or(AppError::NotFound)?;
    if account.totp_enabled {
        return Ok(see_other("/settings/2fa"));
    }
    let mut errors = FieldErrors::default();
    confirm_password(&state, &client, &form.current, "current", &mut errors, lang).await?;
    if !errors.is_empty() {
        return render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &two_factor_page(TwoFactorStep::Start, errors),
        );
    }
    let secret = auth::new_pending_totp_secret(&state.db).await?;
    let mut page = two_factor_page(TwoFactorStep::Scan, FieldErrors::default());
    page.qr_svg = qr_svg(&auth::otpauth_uri(&secret, &account.username), lang)?;
    // Grouped in fours, as authenticator apps show keys.
    page.secret = secret
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    render_status(StatusCode::OK, &page)
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct EnableForm {
    code: String,
}

pub async fn enable_two_factor(
    State(state): State<AppState>,
    Client(client): Client,
    axum::Extension(CurrentSession(token)): axum::Extension<CurrentSession>,
    Form(form): Form<EnableForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let mut errors = FieldErrors::default();
    if !state.auth.attempt(&client) {
        errors.add("code", too_many(lang));
    } else if auth::enable_totp(&state.db, &form.code, &token).await? {
        state.auth.succeeded(&client);
    } else {
        errors.add("code", lang.t("The code is incorrect or has expired."));
    }
    if !errors.is_empty() {
        let step = match auth::account(&state.db).await? {
            Some(a) if !a.totp_enabled => TwoFactorStep::Retry,
            _ => return Ok(see_other("/settings/2fa")),
        };
        return render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &two_factor_page(step, errors),
        );
    }
    tracing::info!("two-factor login turned on");
    Ok(see_other("/settings?msg=twofa_enabled#account-heading"))
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct DisableForm {
    current: String,
}

pub async fn disable_two_factor(
    State(state): State<AppState>,
    Client(client): Client,
    Form(form): Form<DisableForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let mut errors = FieldErrors::default();
    confirm_password(&state, &client, &form.current, "current", &mut errors, lang).await?;
    if !errors.is_empty() {
        let page = two_factor_overview(&state, errors).await?;
        return render_status(StatusCode::UNPROCESSABLE_ENTITY, &page);
    }
    auth::disable_totp(&state.db).await?;
    tracing::info!("two-factor login turned off");
    Ok(see_other("/settings?msg=twofa_disabled#account-heading"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_stays_on_this_site() {
        assert_eq!(
            safe_next("/diary?date=2026-01-01"),
            "/diary?date=2026-01-01"
        );
        for bad in [
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "diary",
            "/login",
            "/a b",
            "",
        ] {
            assert_eq!(safe_next(bad), "/", "{bad}");
        }
        assert_eq!(login_url("/weight"), "/login?next=%2Fweight");
        assert_eq!(login_url("/"), "/login");
    }

    #[test]
    fn client_is_read_through_trusted_proxies_only() {
        let trusted = [IpRange::parse("10.0.0.0/8").unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let xff = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert("x-forwarded-for", HeaderValue::from_str(v).unwrap());
            h
        };
        let h = xff("6.6.6.6, 203.0.113.9");
        assert_eq!(client_ip(ip("10.0.0.2"), &h, &trusted), ip("203.0.113.9"));
        assert_eq!(client_ip(ip("10.0.0.2"), &h, &[]), ip("10.0.0.2"));
        assert_eq!(
            client_ip(ip("198.51.100.1"), &h, &trusted),
            ip("198.51.100.1"),
            "untrusted peers cannot choose their address"
        );
        assert_eq!(
            client_ip(ip("10.0.0.2"), &xff("203.0.113.9, 10.0.0.3"), &trusted),
            ip("203.0.113.9"),
            "chained proxies"
        );
        assert_eq!(
            client_ip(ip("10.0.0.2"), &xff("6.6.6.6, junk"), &trusted),
            ip("10.0.0.2")
        );
        assert_eq!(
            client_ip(ip("10.0.0.2"), &HeaderMap::new(), &trusted),
            ip("10.0.0.2")
        );
        assert_eq!(client_key(ip("2001:db8:1:2:aaaa::1")), "2001:db8:1:2::/64");
        assert_eq!(
            client_key(ip("2001:db8:1:2:bbbb::9")),
            client_key(ip("2001:db8:1:2:aaaa::1"))
        );
        assert_eq!(
            client_key(ip("::ffff:192.0.2.1").to_canonical()),
            "192.0.2.1"
        );
    }

    #[test]
    fn failures_are_limited_per_client_and_in_total() {
        let g = Guard::default();
        for _ in 0..MAX_FAILURES_PER_CLIENT {
            assert!(g.attempt("a"));
        }
        assert!(!g.attempt("a"));
        assert!(g.attempt("b"));
        g.succeeded("a");
        g.succeeded("b");
        assert!(g.attempt("a"));
        g.succeeded("a");
        for i in 0..MAX_FAILURES_TOTAL {
            assert!(g.attempt(&format!("c{i}")));
        }
        assert!(!g.attempt("fresh"));
    }

    #[test]
    fn setup_code_is_stable_until_used() {
        let g = Guard::default();
        assert!(!g.setup_code_matches(""));
        let code = g.setup_code().unwrap();
        assert_eq!(code.len(), 11);
        assert_eq!(g.setup_code().unwrap(), code);
        assert!(g.setup_code_matches(&code.to_lowercase().replace('-', " ")));
        assert!(!g.setup_code_matches("AAAAA-AAAAA") || code == "AAAAA-AAAAA");
        g.clear_setup_code();
        assert!(!g.setup_code_matches(&code));
    }
}
