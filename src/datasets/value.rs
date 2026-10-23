//! Cell values as the tables publish them, and validated food assembly.

use super::DatasetFood;
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient, Nutrients, check_value, kj_to_kcal};

/// Grams of salt per milligram of sodium (salt = sodium × 2.5).
const SALT_G_PER_SODIUM_MG: f64 = 2.5 / 1000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    /// "traces", "Tr" or "< 0,5": present below what could be measured.
    Trace,
    /// Empty, "-", "N" (not determined) or anything unreadable.
    Missing,
}

impl Value {
    /// Reads a published cell. Decimal commas are accepted, and values in
    /// parentheses (estimates in some tables) are taken as given.
    pub fn parse(raw: &str) -> Value {
        let s = raw.trim();
        let lower = s.to_ascii_lowercase();
        if matches!(lower.as_str(), "tr" | "trace" | "traces") || s.starts_with('<') {
            return Value::Trace;
        }
        let s = s
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or(s)
            .trim();
        match s.replace(',', ".").parse::<f64>() {
            Ok(v) if v.is_finite() => Value::Number(v),
            _ => Value::Missing,
        }
    }
}

impl From<Option<f64>> for Value {
    fn from(v: Option<f64>) -> Self {
        match v {
            Some(v) if v.is_finite() => Value::Number(v),
            _ => Value::Missing,
        }
    }
}

/// Builds a [`DatasetFood`], validating every value against the same
/// limits as manual entry. Invalid values are dropped and listed, never
/// adjusted.
pub struct FoodBuilder {
    food: DatasetFood,
}

impl FoodBuilder {
    pub fn new(code: impl Into<String>, basis: Basis) -> Self {
        FoodBuilder {
            food: DatasetFood {
                code: code.into(),
                names: Vec::new(),
                basis,
                nutrients: Nutrients::default(),
                energy_from_kj: false,
                salt_from_sodium: false,
                carbs_include_fiber: false,
                trace: Vec::new(),
                rejected: Vec::new(),
            },
        }
    }

    /// Adds a name; blank names are ignored. Names are cut to the length
    /// foods allow.
    pub fn name(&mut self, lang: Lang, name: &str) -> &mut Self {
        let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
        if !name.is_empty() && !self.food.names.iter().any(|(l, _)| *l == lang) {
            self.food
                .names
                .push((lang, name.chars().take(200).collect()));
        }
        self
    }

    /// Sets a nutrient unless it is already set (so callers can try
    /// preferred sources first). Returns whether a value was taken.
    pub fn set(&mut self, n: Nutrient, v: Value) -> bool {
        if self.food.nutrients.get(n).is_some() || self.food.rejected.contains(&n) {
            return false;
        }
        match v {
            Value::Number(v) => match check_value(n, self.food.basis, v) {
                Ok(v) => self.food.nutrients.set(n, Some(v)),
                Err(_) => self.food.rejected.push(n),
            },
            Value::Trace => {
                self.food.nutrients.set(n, Some(0.0));
                self.food.trace.push(n);
            }
            Value::Missing => return false,
        }
        true
    }

    /// Energy published in kJ only.
    pub fn energy_kj(&mut self, v: Value) {
        let v = match v {
            Value::Number(kj) => Value::Number(kj_to_kcal(kj)),
            other => other,
        };
        if self.set(Nutrient::Energy, v) {
            self.food.energy_from_kj = true;
        }
    }

    /// Salt derived from sodium in mg.
    pub fn sodium_mg(&mut self, v: Value) {
        let v = match v {
            Value::Number(mg) => Value::Number(mg * SALT_G_PER_SODIUM_MG),
            other => other,
        };
        if self.set(Nutrient::Salt, v) {
            self.food.salt_from_sodium = true;
        }
    }

    pub fn carbs_include_fiber(&mut self) -> &mut Self {
        self.food.carbs_include_fiber = true;
        self
    }

    /// The food, or `None` when it has no name.
    pub fn build(self) -> Option<DatasetFood> {
        (!self.food.names.is_empty()).then_some(self.food)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_published_cells() {
        assert_eq!(Value::parse(" 59,7 "), Value::Number(59.7));
        assert_eq!(Value::parse("1140"), Value::Number(1140.0));
        assert_eq!(Value::parse("(0.5)"), Value::Number(0.5));
        assert_eq!(Value::parse("traces"), Value::Trace);
        assert_eq!(Value::parse("Tr"), Value::Trace);
        assert_eq!(Value::parse("< 0,5"), Value::Trace);
        for missing in ["", " - ", "N", "n/a", "NaN", "inf"] {
            assert_eq!(Value::parse(missing), Value::Missing, "{missing:?}");
        }
    }

    #[test]
    fn builder_validates_converts_and_records() {
        let mut b = FoodBuilder::new("1", Basis::Per100g);
        b.name(Lang::En, "  Apple,   raw ");
        b.energy_kj(Value::Number(209.2));
        b.sodium_mg(Value::Number(400.0));
        assert!(b.set(Nutrient::Fat, Value::Trace));
        assert!(
            b.set(Nutrient::Protein, Value::Number(-1.0)),
            "taken, then rejected"
        );
        assert!(
            !b.set(Nutrient::Protein, Value::Number(1.0)),
            "rejected stays rejected"
        );
        assert!(!b.set(Nutrient::Carbs, Value::Missing));
        assert!(b.set(Nutrient::Carbs, Value::Number(11.0)));
        assert!(
            !b.set(Nutrient::Carbs, Value::Number(99.0)),
            "first source wins"
        );
        let f = b.build().unwrap();
        assert_eq!(f.names, [(Lang::En, "Apple, raw".to_string())]);
        assert!((f.nutrients.energy_kcal.unwrap() - 50.0).abs() < 1e-9);
        assert!(f.energy_from_kj);
        assert!((f.nutrients.salt_g.unwrap() - 1.0).abs() < 1e-9);
        assert!(f.salt_from_sodium);
        assert_eq!(f.nutrients.fat_g, Some(0.0));
        assert_eq!(f.trace, [Nutrient::Fat]);
        assert_eq!(f.nutrients.protein_g, None);
        assert_eq!(f.rejected, [Nutrient::Protein]);
        assert_eq!(f.nutrients.carbs_g, Some(11.0));
    }

    #[test]
    fn kcal_found_first_means_energy_is_not_from_kj() {
        let mut b = FoodBuilder::new("1", Basis::Per100g);
        b.name(Lang::En, "x");
        b.set(Nutrient::Energy, Value::Number(100.0));
        b.energy_kj(Value::Number(1000.0));
        let f = b.build().unwrap();
        assert_eq!(f.nutrients.energy_kcal, Some(100.0));
        assert!(!f.energy_from_kj);
    }

    #[test]
    fn nameless_food_is_skipped() {
        assert!(FoodBuilder::new("1", Basis::Per100g).build().is_none());
    }
}
