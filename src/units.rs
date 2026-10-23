//! Unit systems, conversions, calendar dates and form value parsing.

use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::i18n::Lang;

pub const KG_PER_LB: f64 = 0.453_592_37;
pub const CM_PER_IN: f64 = 2.54;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitSystem {
    Metric,
    Imperial,
}

impl UnitSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitSystem::Metric => "metric",
            UnitSystem::Imperial => "imperial",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "metric" => Some(UnitSystem::Metric),
            "imperial" => Some(UnitSystem::Imperial),
            _ => None,
        }
    }

    pub fn weight_unit(self) -> &'static str {
        match self {
            UnitSystem::Metric => "kg",
            UnitSystem::Imperial => "lb",
        }
    }

    pub fn height_unit(self) -> &'static str {
        match self {
            UnitSystem::Metric => "cm",
            UnitSystem::Imperial => "in",
        }
    }

    /// Converts a displayed weight into canonical kilograms.
    pub fn weight_to_kg(self, v: f64) -> f64 {
        match self {
            UnitSystem::Metric => v,
            UnitSystem::Imperial => v * KG_PER_LB,
        }
    }

    pub fn weight_from_kg(self, kg: f64) -> f64 {
        match self {
            UnitSystem::Metric => kg,
            UnitSystem::Imperial => kg / KG_PER_LB,
        }
    }

    pub fn height_to_cm(self, v: f64) -> f64 {
        match self {
            UnitSystem::Metric => v,
            UnitSystem::Imperial => v * CM_PER_IN,
        }
    }

    pub fn height_from_cm(self, cm: f64) -> f64 {
        match self {
            UnitSystem::Metric => cm,
            UnitSystem::Imperial => cm / CM_PER_IN,
        }
    }
}

/// The current calendar date in the given IANA time zone. A diary date is a
/// calendar date in the user's zone, not the server's clock.
pub fn today_in(tz_name: &str) -> Date {
    today_at(jiff::Timestamp::now(), tz_name)
}

pub fn today_at(now: jiff::Timestamp, tz_name: &str) -> Date {
    let tz = TimeZone::get(tz_name).unwrap_or(TimeZone::UTC);
    now.to_zoned(tz).date()
}

pub fn valid_timezone(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && TimeZone::get(name).is_ok()
}

pub fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    // Only accept the canonical form so stored dates stay sortable as text.
    if s.len() != 10 {
        return None;
    }
    s.parse::<Date>().ok()
}

fn weekday_name(d: Date, lang: Lang) -> &'static str {
    const NAMES: [[&str; 7]; 3] = [
        [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ],
        [
            "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
        ],
        [
            "lunes",
            "martes",
            "miércoles",
            "jueves",
            "viernes",
            "sábado",
            "domingo",
        ],
    ];
    NAMES[lang_index(lang)][usize::from(d.weekday().to_monday_zero_offset() as u8)]
}

fn month_name(d: Date, lang: Lang, short: bool) -> &'static str {
    const LONG: [[&str; 12]; 3] = [
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
        [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ],
        [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
    ];
    const SHORT: [[&str; 12]; 3] = [
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ],
        [
            "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
            "nov.", "déc.",
        ],
        [
            "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
        ],
    ];
    let table = if short { &SHORT } else { &LONG };
    table[lang_index(lang)][usize::from(d.month().unsigned_abs()) - 1]
}

fn lang_index(lang: Lang) -> usize {
    match lang {
        Lang::En => 0,
        Lang::Fr => 1,
        Lang::Es => 2,
    }
}

/// E.g. "Thursday, 22 October 2026", "jeudi 22 octobre 2026",
/// "jueves, 22 de octubre de 2026".
pub fn fmt_date_long(d: Date, lang: Lang) -> String {
    let (wd, day, month, year) = (
        weekday_name(d, lang),
        d.day(),
        month_name(d, lang, false),
        d.year(),
    );
    match lang {
        Lang::En => format!("{wd}, {day} {month} {year}"),
        Lang::Fr => format!("{wd} {day} {month} {year}"),
        Lang::Es => format!("{wd}, {day} de {month} de {year}"),
    }
}

/// E.g. "22 Oct 2026", "22 oct. 2026", "22 oct 2026".
pub fn fmt_date_short(d: Date, lang: Lang) -> String {
    format!("{} {} {}", d.day(), month_name(d, lang, true), d.year())
}

/// Why a form value could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    NotANumber,
    TooLong,
    MaxChars(usize),
}

impl InputError {
    /// A predicate completing "<field> …", e.g. "must be a number".
    pub fn message(self, lang: Lang) -> String {
        match self {
            InputError::NotANumber => lang.t("must be a number").to_string(),
            InputError::TooLong => lang.t("is too long").to_string(),
            InputError::MaxChars(n) => lang.f("must be at most {0} characters", &[&n]),
        }
    }

    /// A full sentence naming the field, e.g. "Brand is too long.".
    pub fn for_field(self, lang: Lang, field: &str) -> String {
        lang.f("{0} {1}.", &[&field, &self.message(lang)])
    }
}

/// Parses a user-entered decimal number. Accepts a comma as decimal
/// separator; returns `Ok(None)` for an empty field.
pub fn parse_decimal(input: &str) -> Result<Option<f64>, InputError> {
    let t = input.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() > 32 {
        return Err(InputError::TooLong);
    }
    let normalized = t.replace(',', ".");
    if normalized.matches('.').count() > 1 {
        return Err(InputError::NotANumber);
    }
    let v: f64 = normalized.parse().map_err(|_| InputError::NotANumber)?;
    if !v.is_finite() {
        return Err(InputError::NotANumber);
    }
    Ok(Some(v))
}

/// Trims an optional text field, mapping blank to `None` and enforcing a
/// maximum length in characters.
pub fn clean_text(input: &str, max_chars: usize) -> Result<Option<String>, InputError> {
    let t = input.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.chars().count() > max_chars {
        return Err(InputError::MaxChars(max_chars));
    }
    Ok(Some(t.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn weight_conversion_round_trips() {
        let i = UnitSystem::Imperial;
        assert!((i.weight_to_kg(100.0) - 45.359237).abs() < 1e-9);
        assert!((i.weight_from_kg(i.weight_to_kg(181.5)) - 181.5).abs() < 1e-9);
        assert_eq!(UnitSystem::Metric.weight_to_kg(72.4), 72.4);
        assert!((i.height_to_cm(70.0) - 177.8).abs() < 1e-9);
    }

    #[test]
    fn today_depends_on_time_zone() {
        // 23:30 UTC on 31 Dec is already 1 Jan in Berlin but still 31 Dec in New York.
        let ts: jiff::Timestamp = "2025-12-31T23:30:00Z".parse().unwrap();
        assert_eq!(today_at(ts, "UTC"), date(2025, 12, 31));
        assert_eq!(today_at(ts, "Europe/Berlin"), date(2026, 1, 1));
        assert_eq!(today_at(ts, "America/New_York"), date(2025, 12, 31));
        // Unknown zones fall back to UTC rather than failing.
        assert_eq!(today_at(ts, "Not/AZone"), date(2025, 12, 31));
    }

    #[test]
    fn today_across_dst_change() {
        // Berlin switches to CEST at 01:00 UTC on 29 Mar 2026.
        let ts: jiff::Timestamp = "2026-03-29T22:30:00Z".parse().unwrap();
        assert_eq!(today_at(ts, "Europe/Berlin"), date(2026, 3, 30));
        let ts: jiff::Timestamp = "2026-03-29T21:30:00Z".parse().unwrap();
        assert_eq!(today_at(ts, "Europe/Berlin"), date(2026, 3, 29));
    }

    #[test]
    fn formats_dates_per_language() {
        let d = date(2026, 10, 22);
        assert_eq!(fmt_date_long(d, Lang::En), "Thursday, 22 October 2026");
        assert_eq!(fmt_date_long(d, Lang::Fr), "jeudi 22 octobre 2026");
        assert_eq!(fmt_date_long(d, Lang::Es), "jueves, 22 de octubre de 2026");
        assert_eq!(fmt_date_short(date(2026, 2, 1), Lang::En), "1 Feb 2026");
        assert_eq!(fmt_date_short(date(2026, 2, 1), Lang::Fr), "1 févr. 2026");
        assert_eq!(fmt_date_short(date(2026, 12, 7), Lang::Es), "7 dic 2026");
        assert_eq!(
            fmt_date_long(date(2026, 10, 25), Lang::En),
            "Sunday, 25 October 2026"
        );
    }

    #[test]
    fn parses_dates_strictly() {
        assert_eq!(parse_date("2026-10-22"), Some(date(2026, 10, 22)));
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("2026-1-5"), None);
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("2026-10-22T00:00"), None);
    }

    #[test]
    fn parses_decimals() {
        assert_eq!(parse_decimal(""), Ok(None));
        assert_eq!(parse_decimal(" 12.5 "), Ok(Some(12.5)));
        assert_eq!(parse_decimal("12,5"), Ok(Some(12.5)));
        assert_eq!(parse_decimal("-3"), Ok(Some(-3.0)));
        assert!(parse_decimal("abc").is_err());
        assert!(parse_decimal("1.2.3").is_err());
        assert!(parse_decimal("NaN").is_err());
        assert!(parse_decimal("inf").is_err());
        assert!(parse_decimal("1e400").is_err());
    }

    #[test]
    fn cleans_text() {
        assert_eq!(clean_text("  ", 10), Ok(None));
        assert_eq!(clean_text(" Oats ", 10), Ok(Some("Oats".into())));
        assert!(clean_text("abcdefghijk", 10).is_err());
    }
}
