//! External food data providers.
//!
//! Providers translate their own response formats into [`ProviderFood`];
//! provider-specific types never leave their module. Every provider is
//! optional: the diary and local foods work when all of them are disabled
//! or unreachable.
//!
//! Food names are never machine-translated. Instead, searches and lookups
//! carry the interface [`Lang`] so a provider can match and return product
//! names that were written in that language, falling back to the product's
//! own main name.

pub mod cache;
pub mod off;

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, Nutrients};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A food as reported by a provider, normalized to Korpus units.
#[derive(Clone, Debug, PartialEq)]
pub struct ProviderFood {
    pub source: &'static str,
    pub provider_id: String,
    /// Name in the requested language, or the product's main name.
    pub name: String,
    /// Every name the provider publishes in a supported language, so a
    /// stored food can be shown in whichever language is active later.
    pub names: Vec<(Lang, String)>,
    pub brand: Option<String>,
    pub barcode: Option<String>,
    pub basis: Basis,
    /// The provider did not state the basis explicitly; it was inferred.
    pub basis_inferred: bool,
    pub nutrients: Nutrients,
    /// Energy was only given in kJ and has been converted to kcal.
    pub energy_from_kj: bool,
    pub serving_amount: Option<f64>,
    pub serving_label: Option<String>,
    /// Provider's last-modified date (YYYY-MM-DD), when known.
    pub source_updated_at: Option<String>,
    /// Values present in the provider data but rejected as invalid.
    pub rejected: Vec<Nutrient>,
}

impl ProviderFood {
    /// Notes stored with an imported food that explain how values were
    /// derived, written in the language active at import time.
    pub fn import_notes(&self, lang: Lang) -> Option<String> {
        let mut notes = Vec::new();
        if self.energy_from_kj {
            notes.push(
                lang.t("Energy was provided in kJ and converted to kcal (1 kcal = 4.184 kJ).")
                    .to_string(),
            );
        }
        if self.basis_inferred {
            notes.push(lang.f(
                "Values are {0}; the basis was inferred from the product's package quantity.",
                &[&self.basis.label(lang)],
            ));
        }
        if !self.rejected.is_empty() {
            let names: Vec<&str> = self.rejected.iter().map(|n| n.label(lang)).collect();
            notes.push(lang.f(
                "Invalid source values were ignored for: {0}.",
                &[&names.join(", ")],
            ));
        }
        (!notes.is_empty()).then(|| notes.join(" "))
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProviderPage {
    pub foods: Vec<ProviderFood>,
    pub has_more: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderError {
    Timeout,
    RateLimited,
    Unavailable(String),
    InvalidResponse(String),
}

impl ProviderError {
    /// User-facing explanation, without internal details.
    pub fn message(&self, lang: Lang) -> &'static str {
        match self {
            ProviderError::Timeout => lang.t("the service did not respond in time"),
            ProviderError::RateLimited => {
                lang.t("too many searches in a short time; please wait a minute")
            }
            ProviderError::Unavailable(_) => lang.t("the service is currently unavailable"),
            ProviderError::InvalidResponse(_) => lang.t("the service returned unexpected data"),
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message(Lang::En))
    }
}

pub trait FoodProvider: Send + Sync {
    /// Stable identifier stored in `foods.source`.
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Attribution shown next to this provider's data.
    fn attribution(&self, lang: Lang) -> &'static str;
    fn attribution_url(&self) -> &'static str;
    /// Text search. `lang` selects which language's product names are
    /// matched and returned.
    fn search<'a>(
        &'a self,
        query: &'a str,
        page: usize,
        lang: Lang,
    ) -> BoxFuture<'a, Result<ProviderPage, ProviderError>>;
    /// Fetches one record by provider id, with its name in `lang` if known.
    fn get<'a>(
        &'a self,
        provider_id: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>>;
    /// Exact barcode lookup, if the provider supports it.
    fn lookup_barcode<'a>(
        &'a self,
        barcode: &'a str,
        lang: Lang,
    ) -> BoxFuture<'a, Result<Option<ProviderFood>, ProviderError>>;
}

#[derive(Clone, Default)]
pub struct Providers {
    list: Vec<Arc<dyn FoodProvider>>,
}

impl Providers {
    pub fn new(list: Vec<Arc<dyn FoodProvider>>) -> Self {
        Providers { list }
    }

    pub fn from_config(config: &crate::config::Config) -> Self {
        let mut list: Vec<Arc<dyn FoodProvider>> = Vec::new();
        if config.off_enabled {
            match off::OpenFoodFacts::new(config) {
                Ok(p) => list.push(Arc::new(p)),
                Err(e) => {
                    tracing::error!(error = %e, "could not initialise Open Food Facts provider")
                }
            }
        }
        Providers { list }
    }

    pub fn all(&self) -> &[Arc<dyn FoodProvider>] {
        &self.list
    }

    pub fn find(&self, id: &str) -> Option<&Arc<dyn FoodProvider>> {
        self.list.iter().find(|p| p.id() == id)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}
