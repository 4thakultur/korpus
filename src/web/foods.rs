//! Food detail, manual creation/editing, deletion and provider import.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::{
    AppState, LogContext, current_lang, flash_message, parse_id, render, render_status, see_other,
};
use crate::diary::Meal;
use crate::error::{AppError, AppResult};
use crate::foods::{self, Food, FoodForm, SaveError};
use crate::forms::{FieldErrors, InputField};
use crate::i18n::Lang;
use crate::nutrition::{Nutrient, Nutrients, fmt_amount, fmt_g, fmt_kcal};
use crate::profile;
use crate::units::today_in;

pub struct NutrientRow {
    pub key: &'static str,
    pub label: &'static str,
    pub per100: String,
    pub per_serving: String,
    pub missing: bool,
    pub core: bool,
}

/// One part of the bar showing which macronutrient the energy comes from.
pub struct SplitSegment {
    pub key: &'static str,
    pub label: &'static str,
    /// Start and width in percent of the bar, for SVG attributes.
    pub x: String,
    pub width: String,
    pub pct: u32,
}

/// Share of energy from protein, carbohydrate and fat, using the usual
/// 4/4/9 kcal per gram factors. Empty when a macro is missing or all are 0,
/// so the bar never suggests a split the data does not support.
fn macro_split(n: &Nutrients, lang: Lang) -> Vec<SplitSegment> {
    let parts = [
        (Nutrient::Protein, n.protein_g, 4.0),
        (Nutrient::Carbs, n.carbs_g, 4.0),
        (Nutrient::Fat, n.fat_g, 9.0),
    ];
    let Some(kcal) = parts
        .iter()
        .map(|(_, g, f)| g.map(|g| g * f))
        .collect::<Option<Vec<f64>>>()
    else {
        return Vec::new();
    };
    let total: f64 = kcal.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut x = 0.0;
    parts
        .iter()
        .zip(kcal)
        .map(|((k, _, _), v)| {
            let width = v / total * 100.0;
            let seg = SplitSegment {
                key: k.key(),
                label: k.label(lang),
                x: format!("{x:.2}"),
                width: format!("{width:.2}"),
                pct: width.round() as u32,
            };
            x += width;
            seg
        })
        .collect()
}

#[derive(Template)]
#[template(path = "food_detail.html")]
pub struct FoodPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub food: Food,
    pub basis_label: &'static str,
    pub unit: &'static str,
    pub rows: Vec<NutrientRow>,
    /// Energy per 100 g/ml for the headline number; `None` when missing.
    pub kcal: Option<String>,
    pub split: Vec<SplitSegment>,
    pub missing_core: Vec<&'static str>,
    pub serving_heading: Option<String>,
    pub source_label: &'static str,
    pub source_url: Option<String>,
    pub attribution: Option<&'static str>,
    pub date: String,
    pub meal: String,
    pub amount: String,
    pub meals: [Meal; 4],
    pub errors: FieldErrors,
    pub message: Option<&'static str>,
    pub data_kcal: String,
    pub data_protein: String,
    pub data_carbs: String,
    pub data_fat: String,
    pub serving_amount: String,
    pub back_url: String,
}

/// Suggests a meal from the local time of day.
fn meal_for_hour(hour: i8) -> Meal {
    match hour {
        4..=10 => Meal::Breakfast,
        11..=14 => Meal::Lunch,
        17..=21 => Meal::Dinner,
        _ => Meal::Snacks,
    }
}

fn fmt_value(n: Nutrient, v: f64, lang: Lang) -> String {
    match n {
        Nutrient::Energy => format!("{} kcal", fmt_kcal(v)),
        _ => format!("{} g", lang.num(fmt_g(v))),
    }
}

pub async fn detail_page(
    state: &AppState,
    food: Food,
    date: &str,
    meal: &str,
    amount: &str,
    errors: FieldErrors,
    message: Option<&'static str>,
) -> AppResult<FoodPage> {
    let lang = current_lang();
    let p = profile::load(&state.db).await?;
    let ctx = LogContext::from_parts(Some(date), Some(meal));
    let date = ctx
        .date
        .unwrap_or_else(|| today_in(&p.timezone))
        .to_string();
    let meal = match ctx.meal {
        Some(m) => m,
        None => {
            let tz = jiff::tz::TimeZone::get(&p.timezone).unwrap_or(jiff::tz::TimeZone::UTC);
            meal_for_hour(jiff::Timestamp::now().to_zoned(tz).hour())
        }
    };
    let basis = food.basis();
    let n = food.nutrients();
    let rows = Nutrient::ALL
        .into_iter()
        .map(|k| {
            let v = n.get(k);
            NutrientRow {
                key: k.key(),
                label: k.label(lang),
                per100: v
                    .map(|v| fmt_value(k, v, lang))
                    .unwrap_or_else(|| lang.t("Not available").into()),
                per_serving: match (v, food.serving_amount) {
                    (Some(v), Some(s)) => fmt_value(k, v * s / 100.0, lang),
                    _ => "–".into(),
                },
                missing: v.is_none(),
                core: Nutrient::CORE.contains(&k),
            }
        })
        .collect();
    let serving_heading = food.serving_amount.map(|s| match &food.serving_label {
        Some(l) => lang.f("Per serving ({0})", &[l]),
        None => lang.f(
            "Per serving ({0})",
            &[&format!("{} {}", lang.num(fmt_amount(s)), basis.unit())],
        ),
    });
    let provider = state.providers.find(&food.source);
    let is_off = food.source == crate::providers::off::SOURCE;
    let source_url = food
        .provider_id
        .as_ref()
        // Only a well-formed barcode goes into the link: imported backups
        // can carry any identifier.
        .filter(|id| is_off && foods::is_valid_barcode(id))
        .map(|id| format!("https://world.openfoodfacts.org/product/{id}"));
    let attribution = if is_off {
        Some(
            provider
                .map(|p| p.attribution(lang))
                .unwrap_or(lang.t("Product data © Open Food Facts contributors (ODbL).")),
        )
    } else {
        crate::datasets::Dataset::parse(&food.source).map(|d| d.attribution(lang))
    };
    let data = |v: Option<f64>| v.map(|v| v.to_string()).unwrap_or_default();
    let amount = if amount.trim().is_empty() {
        fmt_amount(food.serving_amount.unwrap_or(100.0))
    } else {
        amount.to_string()
    };
    let back_url = if ctx.is_logging() {
        ctx.url("/search")
    } else {
        "/search".to_string()
    };
    Ok(FoodPage {
        lang,
        title: food.name.clone(),
        nav: "foods",
        basis_label: basis.label(lang),
        unit: basis.unit(),
        rows,
        kcal: n.energy_kcal.map(fmt_kcal),
        split: macro_split(&n, lang),
        missing_core: n
            .missing_core()
            .into_iter()
            .map(|k| k.label(lang))
            .collect(),
        serving_heading,
        source_label: food.source_label(lang),
        source_url,
        attribution,
        date,
        meal: meal.as_str().to_string(),
        amount,
        meals: Meal::ALL,
        errors,
        message,
        data_kcal: data(n.energy_kcal),
        data_protein: data(n.protein_g),
        data_carbs: data(n.carbs_g),
        data_fat: data(n.fat_g),
        serving_amount: food.serving_amount.map(fmt_amount).unwrap_or_default(),
        back_url,
        food,
    })
}

#[derive(Deserialize, Default)]
pub struct DetailQuery {
    date: Option<String>,
    meal: Option<String>,
    msg: Option<String>,
}

pub async fn detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<DetailQuery>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let food = foods::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let page = detail_page(
        &state,
        food,
        q.date.as_deref().unwrap_or(""),
        q.meal.as_deref().unwrap_or(""),
        "",
        FieldErrors::default(),
        flash_message(q.msg.as_deref(), current_lang()),
    )
    .await?;
    Ok(render(&page)?.into_response())
}

#[derive(Template)]
#[template(path = "food_form.html")]
pub struct FoodFormPage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub is_edit: bool,
    pub action: String,
    pub form: FoodForm,
    pub errors: FieldErrors,
    pub duplicate_url: Option<String>,
    pub ctx: LogContext,
    pub cancel_url: String,
    pub provider_note: Option<&'static str>,
}

#[derive(Deserialize, Default)]
pub struct NewQuery {
    date: Option<String>,
    meal: Option<String>,
    name: Option<String>,
    barcode: Option<String>,
}

impl FoodFormPage {
    pub fn nutrient_fields(&self) -> Vec<InputField> {
        let f = &self.form;
        let lang = self.lang;
        let field = |id, label, value: &String, required| InputField {
            id,
            label,
            value: value.clone(),
            required,
        };
        vec![
            field("protein", Nutrient::Protein.label(lang), &f.protein, true),
            field("carbs", Nutrient::Carbs.label(lang), &f.carbs, true),
            field("fat", Nutrient::Fat.label(lang), &f.fat, true),
            field("sugars", lang.t("of which sugars"), &f.sugars, false),
            field(
                "sat_fat",
                lang.t("of which saturated fat"),
                &f.sat_fat,
                false,
            ),
            field("fiber", Nutrient::Fiber.label(lang), &f.fiber, false),
            field("salt", Nutrient::Salt.label(lang), &f.salt, false),
        ]
    }
}

pub async fn new_form(Query(q): Query<NewQuery>) -> AppResult<Response> {
    let ctx = LogContext::from_parts(q.date.as_deref(), q.meal.as_deref());
    let mut form = FoodForm {
        basis: "g".into(),
        energy_unit: "kcal".into(),
        ..Default::default()
    };
    // Prefill from the search that found nothing.
    if let Some(name) = q.name.filter(|n| n.chars().count() <= 200) {
        let barcode = foods::normalize_barcode(&name);
        if foods::is_valid_barcode(&barcode) {
            form.barcode = barcode;
        } else {
            form.name = name.trim().to_string();
        }
    }
    if let Some(b) = q.barcode {
        form.barcode = foods::normalize_barcode(&b);
    }
    let lang = current_lang();
    let page = FoodFormPage {
        lang,
        title: lang.t("Create a food").into(),
        nav: "foods",
        is_edit: false,
        action: ctx.url("/foods"),
        form,
        errors: FieldErrors::default(),
        duplicate_url: None,
        cancel_url: ctx.url("/search"),
        ctx,
        provider_note: None,
    };
    Ok(render(&page)?.into_response())
}

fn save_error_page(page: &mut FoodFormPage, e: SaveError) -> AppResult<()> {
    match e {
        SaveError::DuplicateBarcode(other) => {
            page.errors.add(
                "barcode",
                page.lang.t("Another food already uses this barcode."),
            );
            page.duplicate_url = Some(page.ctx.url(&format!("/foods/{other}")));
            Ok(())
        }
        SaveError::Db(e) => Err(e.into()),
    }
}

pub async fn create(
    State(state): State<AppState>,
    Query(ctxq): Query<super::LogContextQuery>,
    Form(form): Form<FoodForm>,
) -> AppResult<Response> {
    let ctx = LogContext::from_parts(ctxq.date.as_deref(), ctxq.meal.as_deref());
    let lang = current_lang();
    let mut page = FoodFormPage {
        lang,
        title: lang.t("Create a food").into(),
        nav: "foods",
        is_edit: false,
        action: ctx.url("/foods"),
        form: form.clone(),
        errors: FieldErrors::default(),
        duplicate_url: None,
        cancel_url: ctx.url("/search"),
        ctx,
        provider_note: None,
    };
    match foods::validate(&form, lang) {
        Ok(input) => match foods::create(&state.db, &input).await {
            Ok(id) => {
                let sep = if ctx.query().is_empty() { "?" } else { "&" };
                Ok(see_other(&format!(
                    "{}{sep}msg=food_created",
                    ctx.url(&format!("/foods/{id}"))
                )))
            }
            Err(e) => {
                save_error_page(&mut page, e)?;
                render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
            }
        },
        Err(errors) => {
            page.errors = errors;
            render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
        }
    }
}

fn edit_page(food: &Food, form: FoodForm, errors: FieldErrors, lang: Lang) -> FoodFormPage {
    FoodFormPage {
        lang,
        title: lang.f("Edit {0}", &[&food.name]),
        nav: "foods",
        is_edit: true,
        action: format!("/foods/{}", food.id),
        form,
        errors,
        duplicate_url: None,
        ctx: LogContext::default(),
        cancel_url: format!("/foods/{}", food.id),
        provider_note: (!food.is_user_created()).then_some(lang.t(
            "This food was imported. Your changes are kept locally and will not be overwritten when the product is imported again.",
        )),
    }
}

pub async fn edit_form(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let food = foods::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let page = edit_page(
        &food,
        FoodForm::from_food(&food),
        FieldErrors::default(),
        current_lang(),
    );
    Ok(render(&page)?.into_response())
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Form(form): Form<FoodForm>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let food = foods::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let lang = current_lang();
    match foods::validate(&form, lang) {
        Ok(input) => match foods::update(&state.db, id, &input, &food.name).await {
            Ok(true) => Ok(see_other(&format!("/foods/{id}?msg=food_saved"))),
            Ok(false) => Err(AppError::NotFound),
            Err(e) => {
                let mut page = edit_page(&food, form, FieldErrors::default(), lang);
                save_error_page(&mut page, e)?;
                render_status(StatusCode::UNPROCESSABLE_ENTITY, &page)
            }
        },
        Err(errors) => render_status(
            StatusCode::UNPROCESSABLE_ENTITY,
            &edit_page(&food, form, errors, lang),
        ),
    }
}

#[derive(Template)]
#[template(path = "food_delete.html")]
pub struct DeletePage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub food: Food,
    pub references: i64,
}

pub async fn delete_confirm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    let id = parse_id(&id)?;
    let food = foods::get(&state.db, id, current_lang())
        .await?
        .ok_or(AppError::NotFound)?;
    let references = foods::count_diary_references(&state.db, id).await?;
    let lang = current_lang();
    let page = DeletePage {
        lang,
        title: lang.f("Delete {0}", &[&food.name]),
        nav: "foods",
        food,
        references,
    };
    Ok(render(&page)?.into_response())
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<String>) -> AppResult<Response> {
    let id = parse_id(&id)?;
    if !foods::delete(&state.db, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(see_other("/search?msg=food_deleted"))
}

#[derive(Template)]
#[template(path = "notice.html")]
pub struct NoticePage {
    pub lang: Lang,
    pub title: String,
    pub nav: &'static str,
    pub message: String,
    pub links: Vec<(String, String)>,
}

#[derive(Deserialize)]
pub struct ImportForm {
    #[serde(default)]
    source: String,
    #[serde(default)]
    provider_id: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    meal: String,
}

/// Saves a provider product locally (re-fetched from the provider, never
/// trusted from the form) and continues to its detail page.
pub async fn import(
    State(state): State<AppState>,
    Form(form): Form<ImportForm>,
) -> AppResult<Response> {
    let lang = current_lang();
    let ctx = LogContext::from_parts(Some(&form.date), Some(&form.meal));
    if let Some(dataset) = crate::datasets::Dataset::parse(&form.source) {
        return import_from_dataset(&state, dataset, &form.provider_id, ctx, lang).await;
    }
    let provider = state
        .providers
        .find(&form.source)
        .ok_or_else(|| AppError::BadRequest(lang.t("This food source is not enabled.").into()))?;
    if form.provider_id.is_empty() || form.provider_id.len() > 64 {
        return Err(AppError::BadRequest(
            lang.t("Invalid product identifier.").into(),
        ));
    }
    let manual_url = ctx.url("/foods/new");
    match provider.get(&form.provider_id, lang).await {
        Ok(Some(p)) => {
            let id = foods::upsert_from_provider(&state.db, &p, lang).await?;
            let url = ctx.url(&format!("/foods/{id}"));
            let sep = if url.contains('?') { "&" } else { "?" };
            Ok(see_other(&format!("{url}{sep}msg=imported_food")))
        }
        Ok(None) => render_status(
            StatusCode::NOT_FOUND,
            &NoticePage {
                lang,
                title: lang.t("Product not found").into(),
                nav: "foods",
                message: lang.f(
                    "{0} no longer has this product. You can create it manually.",
                    &[&provider.name()],
                ),
                links: vec![(lang.t("Create food manually").into(), manual_url)],
            },
        ),
        Err(e) => {
            tracing::warn!(provider = provider.id(), error = ?e, "provider import failed");
            render_status(
                StatusCode::BAD_GATEWAY,
                &NoticePage {
                    lang,
                    title: lang.f("{0} is unavailable", &[&provider.name()]),
                    nav: "foods",
                    message: lang.f(
                        "The product could not be loaded: {0}. Your diary and saved foods are not affected. You can try again or create the food manually.",
                        &[&e.message(lang)],
                    ),
                    links: vec![
                        (lang.t("Create food manually").into(), manual_url),
                        (lang.t("Back to search").into(), ctx.url("/search")),
                    ],
                },
            )
        }
    }
}

/// Saves a food from a local food composition table (read from the table,
/// never from the form) and continues to its detail page.
async fn import_from_dataset(
    state: &AppState,
    dataset: crate::datasets::Dataset,
    code: &str,
    ctx: LogContext,
    lang: Lang,
) -> AppResult<Response> {
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::BadRequest(
            lang.t("Invalid product identifier.").into(),
        ));
    }
    match crate::datasets::get(&state.db, dataset, code, lang).await? {
        Some(p) => {
            let id = foods::upsert_from_provider(&state.db, &p, lang).await?;
            let url = ctx.url(&format!("/foods/{id}"));
            let sep = if url.contains('?') { "&" } else { "?" };
            Ok(see_other(&format!("{url}{sep}msg=imported_food")))
        }
        // The table was replaced or removed since the search.
        None => render_status(
            StatusCode::NOT_FOUND,
            &NoticePage {
                lang,
                title: lang.t("Product not found").into(),
                nav: "foods",
                message: lang.f(
                    "{0} no longer has this product. You can create it manually.",
                    &[&dataset.name()],
                ),
                links: vec![
                    (lang.t("Create food manually").into(), ctx.url("/foods/new")),
                    (lang.t("Back to search").into(), ctx.url("/search")),
                ],
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meal_suggestion_by_hour() {
        assert_eq!(meal_for_hour(7), Meal::Breakfast);
        assert_eq!(meal_for_hour(12), Meal::Lunch);
        assert_eq!(meal_for_hour(19), Meal::Dinner);
        assert_eq!(meal_for_hour(23), Meal::Snacks);
        assert_eq!(meal_for_hour(15), Meal::Snacks);
    }

    #[test]
    fn macro_split_uses_energy_factors() {
        let n = Nutrients {
            protein_g: Some(10.0),
            carbs_g: Some(10.0),
            fat_g: Some(0.0),
            ..Nutrients::default()
        };
        let s = macro_split(&n, Lang::En);
        assert_eq!(s.iter().map(|s| s.pct).collect::<Vec<_>>(), [50, 50, 0]);
        assert_eq!(s[1].x, "50.00");

        let n = Nutrients {
            protein_g: Some(10.0),
            carbs_g: Some(0.0),
            fat_g: Some(10.0),
            ..Nutrients::default()
        };
        assert_eq!(macro_split(&n, Lang::En)[2].pct, 69);
    }

    #[test]
    fn macro_split_is_empty_without_complete_macros() {
        let missing = Nutrients {
            protein_g: Some(10.0),
            carbs_g: None,
            fat_g: Some(1.0),
            ..Nutrients::default()
        };
        assert!(macro_split(&missing, Lang::En).is_empty());
        let zero = Nutrients {
            protein_g: Some(0.0),
            carbs_g: Some(0.0),
            fat_g: Some(0.0),
            ..Nutrients::default()
        };
        assert!(macro_split(&zero, Lang::En).is_empty());
    }
}
