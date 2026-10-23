//! First-run setup, settings, export, backup, import and food composition
//! tables.

use askama::Template;
use axum::Form;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{
    AppState, Theme, browser_lang, current_lang, current_theme, flash_message, render,
    render_status, see_other,
};
use crate::backup::{self, Export, MAX_IMPORT_BYTES};
use crate::countries::{self, CountryOption};
use crate::datasets::{self, Dataset, UploadFile};
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
    /// Country code, or empty when not set.
    pub country: String,
}

struct ValidProfile {
    units: UnitSystem,
    timezone: String,
    language: Option<Lang>,
    country: Option<&'static str>,
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
    let country = match form.country.trim() {
        "" => Some(None),
        code => countries::parse(code).map(Some),
    };
    if country.is_none() {
        errors.add("country", lang.t("Choose a country from the list."));
    }
    match (units, language, country) {
        (Some(units), Some(language), Some(country)) if errors.is_empty() => Ok(ValidProfile {
            units,
            timezone: tz.to_string(),
            language,
            country,
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
    pub countries: Vec<CountryOption>,
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
            country: p.country.unwrap_or_default(),
        },
        errors: FieldErrors::default(),
        languages: language_options(lang),
        countries: countries::options(lang),
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
            profile::save(&state.db, v.units, &v.timezone, v.language, v.country).await?;
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
                countries: countries::options(lang),
            },
        ),
    }
}

/// One food composition table in Settings.
pub struct DatasetView {
    pub id: &'static str,
    pub name: &'static str,
    pub country: &'static str,
    pub attribution: &'static str,
    pub download_url: &'static str,
    pub file_hint: &'static str,
    pub accept: &'static str,
    pub multiple: bool,
    /// Number of foods and import date, when imported.
    pub status: Option<String>,
    /// Search uses this table.
    pub active: bool,
    /// The preferred table for the profile's country.
    pub preferred: bool,
    /// Why the last upload for this table failed.
    pub error: Option<String>,
}

/// A failed dataset upload, shown next to its table.
pub struct DatasetError {
    pub dataset: Dataset,
    pub message: String,
}

async fn dataset_views(
    state: &AppState,
    lang: Lang,
    country: Option<&str>,
    error: Option<DatasetError>,
) -> AppResult<(Vec<DatasetView>, Option<&'static str>)> {
    let installed = datasets::installed(&state.db).await?;
    let active = datasets::choose(country, &installed);
    let order = datasets::preference(country);
    let views = order
        .iter()
        .map(|d| DatasetView {
            id: d.id(),
            name: d.name(),
            country: d.country_name(lang),
            attribution: d.attribution(lang),
            download_url: d.download_url(),
            file_hint: d.file_hint(lang),
            accept: d.accept(),
            multiple: d.multiple_files(),
            status: installed.iter().find(|i| i.dataset == *d).map(|i| {
                lang.f(
                    "{0} foods, imported {1}",
                    &[&i.food_count, &i.imported_at.get(..10).unwrap_or("")],
                )
            }),
            active: active == Some(*d),
            preferred: country.is_some() && order[0] == *d,
            error: error
                .as_ref()
                .filter(|e| e.dataset == *d)
                .map(|e| e.message.clone()),
        })
        .collect();
    Ok((views, active.map(Dataset::name)))
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
    pub countries: Vec<CountryOption>,
    /// The saved country's name, if any.
    pub country_name: Option<&'static str>,
    pub datasets: Vec<DatasetView>,
    /// Name of the table search uses, if any is imported.
    pub active_dataset: Option<&'static str>,
    pub theme: Theme,
    pub themes: [Theme; 3],
}

async fn settings_page(
    state: &AppState,
    lang: Lang,
    form: Option<ProfileForm>,
    errors: FieldErrors,
    message: Option<&'static str>,
    import_error: Option<String>,
    dataset_error: Option<DatasetError>,
) -> AppResult<SettingsPage> {
    let p = profile::load(&state.db).await?;
    let country = p.country();
    let (datasets, active_dataset) = dataset_views(state, lang, country, dataset_error).await?;
    Ok(SettingsPage {
        lang,
        title: lang.t("Settings").into(),
        nav: "settings",
        form: form.unwrap_or(ProfileForm {
            unit_system: p.unit_system,
            timezone: p.timezone,
            language: p.language.unwrap_or_default(),
            country: p.country.clone().unwrap_or_default(),
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
        countries: countries::options(lang),
        country_name: country.and_then(|c| countries::name(c, lang)),
        datasets,
        active_dataset,
        theme: current_theme(),
        themes: Theme::ALL,
    })
}

#[derive(Deserialize)]
pub struct AppearanceForm {
    #[serde(default)]
    theme: String,
}

/// Stores the light/dark choice for this browser only.
pub async fn appearance(Form(form): Form<AppearanceForm>) -> AppResult<Response> {
    let lang = current_lang();
    let theme = Theme::parse(&form.theme)
        .ok_or_else(|| AppError::BadRequest(lang.t("Choose an appearance.").into()))?;
    let mut res = see_other("/settings?msg=appearance_saved");
    res.headers_mut()
        .insert(header::SET_COOKIE, theme.set_cookie());
    Ok(res)
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
            profile::save(&state.db, v.units, &v.timezone, v.language, v.country).await?;
            Ok(see_other("/settings?msg=settings_saved"))
        }
        Err(errors) => {
            let page = settings_page(&state, lang, Some(form), errors, None, None, None).await?;
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
            let page = settings_page(
                &state,
                lang,
                None,
                FieldErrors::default(),
                None,
                Some(msg),
                None,
            )
            .await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

fn dataset_from_path(id: &str) -> AppResult<Dataset> {
    Dataset::parse(id).ok_or(AppError::NotFound)
}

/// Every uploaded file of a dataset form, within the size limit.
async fn read_dataset_upload(
    mut multipart: Multipart,
    lang: Lang,
) -> Result<Vec<UploadFile>, String> {
    let unreadable = || {
        lang.t("The upload could not be read (is it too large?).")
            .to_string()
    };
    let mut files = Vec::new();
    let mut total = 0;
    while let Some(field) = multipart.next_field().await.map_err(|_| unreadable())? {
        if field.name() != Some("file") {
            continue;
        }
        let name = field.file_name().unwrap_or("upload").to_string();
        let bytes = field.bytes().await.map_err(|_| unreadable())?;
        total += bytes.len();
        if total > datasets::MAX_UPLOAD_BYTES {
            return Err(lang.t("The file is too large.").into());
        }
        if !bytes.is_empty() {
            files.push(UploadFile {
                name,
                bytes: bytes.to_vec(),
            });
        }
    }
    Ok(files)
}

/// Imports (or replaces) a food composition table from the publisher's
/// file(s). Parsing runs on a blocking thread; the previous version stays
/// in place if anything fails.
pub async fn import_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
    multipart: Multipart,
) -> AppResult<Response> {
    let lang = current_lang();
    let dataset = dataset_from_path(&id)?;
    let parsed = match read_dataset_upload(multipart, lang).await {
        Ok(files) => tokio::task::spawn_blocking(move || dataset.parse_files(&files))
            .await
            .map_err(|e| AppError::Internal(format!("dataset import task: {e}")))?
            .map_err(|e| {
                tracing::warn!(dataset = dataset.id(), error = %e, "dataset import rejected");
                e.message(dataset, lang)
            }),
        Err(msg) => Err(msg),
    };
    match parsed {
        Ok(foods) => {
            let stored = datasets::replace(&state.db, dataset, &foods).await?;
            tracing::info!(dataset = dataset.id(), foods = stored, "dataset imported");
            Ok(see_other("/settings?msg=dataset_imported#datasets-heading"))
        }
        Err(message) => {
            let page = settings_page(
                &state,
                lang,
                None,
                FieldErrors::default(),
                None,
                None,
                Some(DatasetError { dataset, message }),
            )
            .await?;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

/// Removes an imported table. Foods already saved from it are kept.
pub async fn remove_dataset(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let dataset = dataset_from_path(&id)?;
    datasets::remove(&state.db, dataset).await?;
    Ok(see_other("/settings?msg=dataset_removed#datasets-heading"))
}
