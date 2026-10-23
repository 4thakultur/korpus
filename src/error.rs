//! Application error type and its HTTP rendering.

use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::i18n::Lang;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("template error: {0}")]
    Template(#[from] askama::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage<'a> {
    lang: Lang,
    title: &'a str,
    message: &'a str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Messages carried by the error were already written in this language.
        let lang = crate::web::current_lang();
        let (status, title, message) = match &self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                lang.t("Not found"),
                lang.t("The page or item you were looking for does not exist. It may have been deleted."),
            ),
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                lang.t("Invalid request"),
                msg.as_str(),
            ),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, lang.t("Request blocked"), msg.as_str()),
            AppError::Database(_) | AppError::Template(_) | AppError::Internal(_) => {
                // Details go to the log only; they may contain user data or paths.
                tracing::error!(error = %self, "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    lang.t("Something went wrong"),
                    lang.t("Korpus could not complete this request. Your data was not changed unless a confirmation was shown. Check the server log for details."),
                )
            }
        };
        let page = ErrorPage {
            lang,
            title,
            message,
        };
        match page.render() {
            Ok(body) => (status, Html(body)).into_response(),
            Err(_) => (status, message.to_string()).into_response(),
        }
    }
}
