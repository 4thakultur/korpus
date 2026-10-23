//! Nutrition values, bases, scaling and validation.
//!
//! All stored nutrient values are expressed per 100 units of the food's
//! [`Basis`]. Values are kept as `f64` and only rounded for display, so
//! repeated scaling or summing never accumulates rounding error.

use std::fmt;

use crate::i18n::Lang;

pub const KJ_PER_KCAL: f64 = 4.184;

/// Whether nutrient values are given per 100 g or per 100 ml.
///
/// Grams and millilitres are never converted into each other: that would
/// require a density the data sources do not reliably provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Per100g,
    Per100ml,
}

impl Basis {
    pub fn as_str(self) -> &'static str {
        match self {
            Basis::Per100g => "g",
            Basis::Per100ml => "ml",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "g" => Some(Basis::Per100g),
            "ml" => Some(Basis::Per100ml),
            _ => None,
        }
    }

    /// Unit of an amount eaten, e.g. "g".
    pub fn unit(self) -> &'static str {
        self.as_str()
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Basis::Per100g => lang.t("per 100 g"),
            Basis::Per100ml => lang.t("per 100 ml"),
        }
    }
}

pub fn kj_to_kcal(kj: f64) -> f64 {
    kj / KJ_PER_KCAL
}

pub fn kcal_to_kj(kcal: f64) -> f64 {
    kcal * KJ_PER_KCAL
}

/// Nutrient amounts. `None` means the value is unknown — never zero.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Nutrients {
    pub energy_kcal: Option<f64>,
    pub protein_g: Option<f64>,
    pub carbs_g: Option<f64>,
    pub fat_g: Option<f64>,
    pub fiber_g: Option<f64>,
    pub sugars_g: Option<f64>,
    pub sat_fat_g: Option<f64>,
    pub salt_g: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nutrient {
    Energy,
    Protein,
    Carbs,
    Fat,
    Fiber,
    Sugars,
    SatFat,
    Salt,
}

impl Nutrient {
    pub const CORE: [Nutrient; 4] = [
        Nutrient::Energy,
        Nutrient::Protein,
        Nutrient::Carbs,
        Nutrient::Fat,
    ];

    pub const ALL: [Nutrient; 8] = [
        Nutrient::Energy,
        Nutrient::Protein,
        Nutrient::Carbs,
        Nutrient::Fat,
        Nutrient::Fiber,
        Nutrient::Sugars,
        Nutrient::SatFat,
        Nutrient::Salt,
    ];

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Nutrient::Energy => lang.t("Energy"),
            Nutrient::Protein => lang.t("Protein"),
            Nutrient::Carbs => lang.t("Carbohydrate"),
            Nutrient::Fat => lang.t("Fat"),
            Nutrient::Fiber => lang.t("Fibre"),
            Nutrient::Sugars => lang.t("Sugars"),
            Nutrient::SatFat => lang.t("Saturated fat"),
            Nutrient::Salt => lang.t("Salt"),
        }
    }

    /// Stable identifier, used as a CSS class so each nutrient keeps one
    /// colour across the interface.
    pub fn key(self) -> &'static str {
        match self {
            Nutrient::Energy => "energy",
            Nutrient::Protein => "protein",
            Nutrient::Carbs => "carbs",
            Nutrient::Fat => "fat",
            Nutrient::Fiber => "fiber",
            Nutrient::Sugars => "sugars",
            Nutrient::SatFat => "sat-fat",
            Nutrient::Salt => "salt",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Nutrient::Energy => "kcal",
            _ => "g",
        }
    }

    /// Largest plausible value per 100 units of `basis`.
    ///
    /// Per 100 g no macronutrient can exceed 100 g and energy is bounded by
    /// pure fat (~900 kcal). Per 100 ml dense liquids (syrups, honey) can
    /// exceed 100 g, so the bounds are looser.
    pub fn max_per_100(self, basis: Basis) -> f64 {
        match (self, basis) {
            (Nutrient::Energy, Basis::Per100g) => 950.0,
            (Nutrient::Energy, Basis::Per100ml) => 1200.0,
            (_, Basis::Per100g) => 100.0,
            (_, Basis::Per100ml) => 200.0,
        }
    }
}

impl fmt::Display for Nutrient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label(Lang::En))
    }
}

impl Nutrients {
    pub fn get(&self, n: Nutrient) -> Option<f64> {
        match n {
            Nutrient::Energy => self.energy_kcal,
            Nutrient::Protein => self.protein_g,
            Nutrient::Carbs => self.carbs_g,
            Nutrient::Fat => self.fat_g,
            Nutrient::Fiber => self.fiber_g,
            Nutrient::Sugars => self.sugars_g,
            Nutrient::SatFat => self.sat_fat_g,
            Nutrient::Salt => self.salt_g,
        }
    }

    pub fn set(&mut self, n: Nutrient, value: Option<f64>) {
        let slot = match n {
            Nutrient::Energy => &mut self.energy_kcal,
            Nutrient::Protein => &mut self.protein_g,
            Nutrient::Carbs => &mut self.carbs_g,
            Nutrient::Fat => &mut self.fat_g,
            Nutrient::Fiber => &mut self.fiber_g,
            Nutrient::Sugars => &mut self.sugars_g,
            Nutrient::SatFat => &mut self.sat_fat_g,
            Nutrient::Salt => &mut self.salt_g,
        };
        *slot = value;
    }

    /// Nutrients in `amount` units, given values per 100 units.
    pub fn scaled(&self, amount: f64) -> Nutrients {
        let f = amount / 100.0;
        let s = |v: Option<f64>| v.map(|v| v * f);
        Nutrients {
            energy_kcal: s(self.energy_kcal),
            protein_g: s(self.protein_g),
            carbs_g: s(self.carbs_g),
            fat_g: s(self.fat_g),
            fiber_g: s(self.fiber_g),
            sugars_g: s(self.sugars_g),
            sat_fat_g: s(self.sat_fat_g),
            salt_g: s(self.salt_g),
        }
    }

    /// Core nutrients (energy, protein, carbohydrate, fat) that are unknown.
    pub fn missing_core(&self) -> Vec<Nutrient> {
        Nutrient::CORE
            .into_iter()
            .filter(|n| self.get(*n).is_none())
            .collect()
    }

    pub fn core_complete(&self) -> bool {
        self.missing_core().is_empty()
    }
}

/// Why a nutrient value was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueError {
    NotFinite,
    Negative,
    Implausible { max: f64 },
}

impl ValueError {
    /// A predicate completing "<nutrient> …", e.g. "cannot be negative".
    pub fn message(self, lang: Lang) -> String {
        match self {
            ValueError::NotFinite => lang.t("must be a number").to_string(),
            ValueError::Negative => lang.t("cannot be negative").to_string(),
            ValueError::Implausible { max } => lang.f("must not exceed {0}", &[&max]),
        }
    }
}

/// Checks one per-100 nutrient value. Values are never adjusted, only
/// accepted or rejected.
pub fn check_value(n: Nutrient, basis: Basis, value: f64) -> Result<f64, ValueError> {
    if !value.is_finite() {
        return Err(ValueError::NotFinite);
    }
    if value < 0.0 {
        return Err(ValueError::Negative);
    }
    let max = n.max_per_100(basis);
    if value > max {
        return Err(ValueError::Implausible { max });
    }
    Ok(value)
}

/// Sum of several entries. A nutrient total is `incomplete` when at least
/// one contributing entry had no value for it, so the UI can say the total
/// is a lower bound instead of pretending it is exact.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub energy_kcal: f64,
    pub protein_g: f64,
    pub carbs_g: f64,
    pub fat_g: f64,
    pub incomplete: bool,
}

impl Totals {
    pub fn add(&mut self, n: &Nutrients) {
        let mut missing = false;
        let mut sum = |acc: &mut f64, v: Option<f64>| match v {
            Some(v) => *acc += v,
            None => missing = true,
        };
        sum(&mut self.energy_kcal, n.energy_kcal);
        sum(&mut self.protein_g, n.protein_g);
        sum(&mut self.carbs_g, n.carbs_g);
        sum(&mut self.fat_g, n.fat_g);
        self.incomplete |= missing;
    }

    pub fn merge(&mut self, other: &Totals) {
        self.energy_kcal += other.energy_kcal;
        self.protein_g += other.protein_g;
        self.carbs_g += other.carbs_g;
        self.fat_g += other.fat_g;
        self.incomplete |= other.incomplete;
    }
}

/// Rounds energy for display: whole kcal.
pub fn fmt_kcal(v: f64) -> String {
    format!("{:.0}", round_half_away(v, 0))
}

/// Rounds a gram value for display: one decimal place.
pub fn fmt_g(v: f64) -> String {
    format!("{:.1}", round_half_away(v, 1))
}

pub fn fmt_opt_kcal(v: Option<f64>) -> String {
    v.map(fmt_kcal).unwrap_or_else(|| "–".to_string())
}

pub fn fmt_opt_g(v: Option<f64>) -> String {
    v.map(fmt_g).unwrap_or_else(|| "–".to_string())
}

/// Formats an amount without trailing zeros (e.g. 150, 12.5).
pub fn fmt_amount(v: f64) -> String {
    let s = format!("{:.1}", round_half_away(v, 1));
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

fn round_half_away(v: f64, decimals: i32) -> f64 {
    let p = 10f64.powi(decimals);
    let r = (v * p).round() / p;
    // Avoid printing "-0".
    if r == 0.0 { 0.0 } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Nutrients {
        Nutrients {
            energy_kcal: Some(250.0),
            protein_g: Some(10.0),
            carbs_g: Some(30.0),
            fat_g: Some(9.5),
            fiber_g: None,
            sugars_g: Some(5.0),
            sat_fat_g: None,
            salt_g: Some(1.2),
        }
    }

    #[test]
    fn scales_by_amount() {
        let s = sample().scaled(150.0);
        assert_eq!(s.energy_kcal, Some(375.0));
        assert_eq!(s.protein_g, Some(15.0));
        assert_eq!(s.carbs_g, Some(45.0));
        assert_eq!(s.fat_g, Some(14.25));
        assert_eq!(s.fiber_g, None, "unknown values stay unknown");
        assert!((s.salt_g.unwrap() - 1.8).abs() < 1e-9);
    }

    #[test]
    fn scaling_by_100_is_identity_and_small_amounts_work() {
        assert_eq!(sample().scaled(100.0), sample());
        let tiny = sample().scaled(0.5);
        assert!((tiny.energy_kcal.unwrap() - 1.25).abs() < 1e-12);
    }

    #[test]
    fn kj_kcal_conversion() {
        assert!((kj_to_kcal(4.184) - 1.0).abs() < 1e-12);
        assert!((kj_to_kcal(1046.0) - 250.0).abs() < 1e-9);
        assert!((kcal_to_kj(100.0) - 418.4).abs() < 1e-9);
        assert!((kj_to_kcal(kcal_to_kj(123.4)) - 123.4).abs() < 1e-9);
    }

    #[test]
    fn missing_core_reports_unknown_fields() {
        let mut n = sample();
        assert!(n.core_complete());
        n.protein_g = None;
        n.energy_kcal = None;
        assert_eq!(n.missing_core(), vec![Nutrient::Energy, Nutrient::Protein]);
    }

    #[test]
    fn validates_values() {
        let g = Basis::Per100g;
        assert_eq!(check_value(Nutrient::Fat, g, 12.0), Ok(12.0));
        assert_eq!(check_value(Nutrient::Fat, g, 0.0), Ok(0.0));
        assert_eq!(
            check_value(Nutrient::Fat, g, -1.0),
            Err(ValueError::Negative)
        );
        assert_eq!(
            check_value(Nutrient::Fat, g, f64::NAN),
            Err(ValueError::NotFinite)
        );
        assert_eq!(
            check_value(Nutrient::Energy, g, f64::INFINITY),
            Err(ValueError::NotFinite)
        );
        assert!(matches!(
            check_value(Nutrient::Protein, g, 100.5),
            Err(ValueError::Implausible { .. })
        ));
        assert!(matches!(
            check_value(Nutrient::Energy, g, 2000.0),
            Err(ValueError::Implausible { .. })
        ));
        // Dense liquids may exceed 100 g per 100 ml.
        assert_eq!(
            check_value(Nutrient::Carbs, Basis::Per100ml, 115.0),
            Ok(115.0)
        );
    }

    #[test]
    fn totals_sum_and_flag_missing() {
        let mut t = Totals::default();
        t.add(&sample().scaled(200.0));
        t.add(&sample().scaled(50.0));
        assert!((t.energy_kcal - 625.0).abs() < 1e-9);
        assert!((t.fat_g - 23.75).abs() < 1e-9);
        assert!(
            !t.incomplete,
            "optional nutrients do not affect completeness"
        );

        let mut partial = sample();
        partial.carbs_g = None;
        t.add(&partial);
        assert!(t.incomplete);
        assert!((t.carbs_g - 75.0).abs() < 1e-9);
    }

    #[test]
    fn totals_merge() {
        let mut a = Totals::default();
        a.add(&sample());
        let mut b = Totals::default();
        b.add(&Nutrients::default());
        a.merge(&b);
        assert!(a.incomplete);
        assert_eq!(a.energy_kcal, 250.0);
    }

    #[test]
    fn display_rounding() {
        assert_eq!(fmt_kcal(374.5), "375");
        assert_eq!(fmt_kcal(-0.2), "0");
        assert_eq!(fmt_g(14.25), "14.3");
        assert_eq!(fmt_g(0.04), "0.0");
        assert_eq!(fmt_opt_g(None), "–");
        assert_eq!(fmt_amount(150.0), "150");
        assert_eq!(fmt_amount(12.5), "12.5");
    }

    #[test]
    fn basis_round_trip() {
        for b in [Basis::Per100g, Basis::Per100ml] {
            assert_eq!(Basis::parse(b.as_str()), Some(b));
        }
        assert_eq!(Basis::parse("oz"), None);
    }
}
