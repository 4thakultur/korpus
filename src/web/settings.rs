//! First-run setup, settings, export, backup and import.

use askama::Template;
use axum::Form;
use axum::extract::{Multipart, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{
    AppState, browser_lang, current_lang, flash_message, render, render_status, see_other,
};
use crate::backup::{self, Export, MAX_IMPORT_BYTES};
use crate::error::{AppError, AppResult};
use crate::forms::FieldErrors;
use crate::i18n::Lang;
use crate::profile;
use crate::units::{UnitSystem, valid_timezone};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProfileForm {
    pub unit_system: String,
    pub timezone: String,
    /// Language code, or empty to follow the browser.
    pub language: String,
}

struct ValidProfile {
    units: UnitSystem,
    timezone: String,
    language: Option<Lang>,
}

fn validate(form: &ProfileForm, lang: Lang) -> Result<ValidProfile, FieldErrors> {
    let mut errors = FieldErrors::default();
    let units = UnitSystem::parse(&form.unit_system);
    if units.is_none() {
        errors.add("unit_system", lang.t("Choose metric or imperial units."));
    }
    let tz = form.timezone.trim();
    if !valid_timezone(tz) {
        errors.add(
            "timezone",
            lang.t("Enter a time zone name such as Europe/Berlin, America/New_York or UTC."),
        );
    }
    let language = match form.language.as_str() {
        "" => Some(None),
        code => Lang::parse(code).map(Some),
    };
    if language.is_none() {
        errors.add("language", lang.t("Choose a language."));
    }
    match (units, language) {
        (Some(units), Some(language)) if errors.is_empty() => Ok(ValidProfile {
            units,
            timezone: tz.to_string(),
            language,
        }),
        _ => Err(errors),
    }
}

/// The language a just-saved form switches to, so its error page is not
/// shown in the old language.
fn form_lang(form: &ProfileForm, headers: &HeaderMap) -> Lang {
    match form.language.as_str() {
        "" => browser_lang(headers),
        code => Lang::parse(code).unwrap_or_else(current_lang),
    }
}

pub struct LanguageOption {
    /// Empty for "follow the browser".
    pub code: &'static str,
    pub label: &'static str,
}

/// Options for the language select, "follow the browser" first.
pub fn language_options(lang: Lang) -> Vec<LanguageOption> {
    let mut options = vec![LanguageOption {
        code: "",
        label: lang.t("Same as browser"),
    }];
    options.extend(Lang::ALL.iter().map(|l| LanguageOption {
        code: l.code(),
        label: l.native_name(),
    }));
    options
}

#[derive(Template)]
#[template(path = "setup.html")]
pub struct SetupPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub form: ProfileForm,
    pub errors: FieldErrors,
    pub languages: Vec<LanguageOption>,
}

pub async fn setup_form(State(state): State<AppState>) -> AppResult<Response> {
    let p = profile::load(&state.db).await?;
    if p.setup_complete {
        return Ok(see_other("/settings"));
    }
    let lang = current_lang();
    let page = SetupPage {
        lang,
        title: lang.t("Welcome to Korpus").into(),
        nav: "",
        form: ProfileForm {
            unit_system: "metric".into(),
            timezone: state.config.default_timezone.clone(),
            language: p.language.unwrap_or_default(),
        },
        errors: FieldErrors::default(),
        languages: language_options(lang),
    };
    Ok(render(&page)?.into_response())
}

pub async fn setup_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ProfileForm>,
) -> AppResult<Response> {
    let lang = form_lang(&form, &headers);
    match validate(&form, lang) {
        Ok(v) => {
            profile::save(&state.db, v.units, &v.timezone, v.language).await?;
            Ok(see_other("/goals?setup=1"))
        }
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &SetupPage {
                lang,
                title: lang.t("Welcome to Korpus").into(),
                nav: "",
                form,
                errors,
                languages: language_options(lang),
            },
        ),
    }
}

pub struct ProviderInfo {
    pub name: &'static str,
    pub attribution: &'static str,
    pub url: &'static str,
}

#[derive(Template)]
#[template(path = "settings.html")]
pub struct SettingsPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub form: ProfileForm,
    pub errors: FieldErrors,
    pub message: Option<&'static str>,
    pub import_error: Option<String>,
    pub providers: Vec<ProviderInfo>,
    pub version: &'static str,
    pub languages: Vec<LanguageOption>,
}

async fn settings_page(
    state: &AppState,
    lang: Lang,
    form: Option<ProfileForm>,
    errors: FieldErrors,
    message: Option<&'static str>,
    import_error: Option<String>,
) -> AppResult<SettingsPage> {
    let p = profile::load(&state.db).await?;
    Ok(SettingsPage {
        lang,
        title: lang.t("Settings").into(),
        nav: "settings",
        form: form.unwrap_or(ProfileForm {
            unit_system: p.unit_system,
            timezone: p.timezone,
            language: p.language.unwrap_or_default(),
        }),
        errors,
        message,
        import_error,
        providers: state
            .providers
            .all()
            .iter()
            .map(|p| ProviderInfo {
                name: p.name(),
                attribution: p.attribution(lang),
                url: p.attribution_url(),
            })
            .collect(),
        version: env!("CARGO_PKG_VERSION"),
        languages: language_options(lang),
    })
}

#[derive(Deserialize)]
pub struct MsgQuery {
    msg: Option<String>,
}

pub async fn page(State(state): State<AppState>, Query(q): Query<MsgQuery>) -> AppResult<Response> {
    let lang = current_lang();
    let page = settings_page(
        &state,
        lang,
        None,
        FieldErrors::default(),
        flash_message(q.msg.as_deref(), lang),
        None,
    )
    .await?;
    Ok(render(&page)?.into_response())
}

pub async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ProfileForm>,
) -> AppResult<Response> {
    let lang = form_lang(&form, &headers);
    match validate(&form, lang) {
        Ok(v) => {
            profile::save(&state.db, v.units, &v.timezone, v.language).await?;
            Ok(see_other("/settings?msg=settings_saved"))
        }
        Err(errors) => {
            let page = settings_page(&state, lang, Some(form), errors, None, None).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

fn download(content_type: &'static str, filename: String, body: impl IntoResponse) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response()
}

fn stamp() -> String {
    jiff::Timestamp::now().strftime("%Y%m%d-%H%M%S").to_string()
}

pub async fn export_json(State(state): State<AppState>) -> AppResult<Response> {
    let export = backup::export(&state.db).await?;
    let body = serde_json::to_vec_pretty(&export)
        .map_err(|e| AppError::Internal(format!("serializing export: {e}")))?;
    Ok(download(
        "application/json",
        format!("korpus-export-{}.json", stamp()),
        body,
    ))
}

pub async fn export_csv(State(state): State<AppState>) -> AppResult<Response> {
    let export = backup::export(&state.db).await?;
    Ok(download(
        "text/csv; charset=utf-8",
        format!("korpus-diary-{}.csv", stamp()),
        backup::diary_csv(&export.diary_entries),
    ))
}

pub async fn backup(State(state): State<AppState>) -> AppResult<Response> {
    let bytes = backup::sqlite_snapshot(&state.db).await?;
    Ok(download(
        "application/vnd.sqlite3",
        format!("korpus-backup-{}.db", stamp()),
        bytes,
    ))
}

async fn read_upload(mut multipart: Multipart, lang: Lang) -> Result<Vec<u8>, String> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| lang.t("The upload could not be read.").to_string())?
    {
        if field.name() == Some("file") {
            let bytes = field.bytes().await.map_err(|_| {
                lang.t("The upload could not be read (is it too large?).")
                    .to_string()
            })?;
            if bytes.is_empty() {
                return Err(lang.t("Choose an export file to import.").into());
            }
            if bytes.len() > MAX_IMPORT_BYTES {
                return Err(lang.t("The file is too large.").into());
            }
            return Ok(bytes.to_vec());
        }
    }
    Err(lang.t("Choose an export file to import.").into())
}

pub async fn import(State(state): State<AppState>, multipart: Multipart) -> AppResult<Response> {
    let lang = current_lang();
    let result = match read_upload(multipart, lang).await {
        Ok(bytes) => match serde_json::from_slice::<Export>(&bytes) {
            Ok(export) => match backup::import(&state.db, &export, lang).await {
                Ok(summary) => {
                    tracing::info!(
                        foods = summary.foods,
                        diary_entries = summary.diary_entries,
                        weight_entries = summary.weight_entries,
                        "import complete"
                    );
                    Ok(())
                }
                Err(AppError::BadRequest(msg)) => Err(msg),
                Err(e) => return Err(e),
            },
            Err(_) => Err(lang.t("The file is not a valid Korpus JSON export.").into()),
        },
        Err(msg) => Err(msg),
    };
    match result {
        Ok(()) => Ok(see_other("/settings?msg=imported")),
        Err(msg) => {
            let page =
                settings_page(&state, lang, None, FieldErrors::default(), None, Some(msg)).await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}
