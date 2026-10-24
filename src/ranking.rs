//! Relevance ranking shared by every search: saved foods, food composition
//! tables and provider results.
//!
//! Candidates are found by simple word matching (or by the provider's own
//! search, which matches any query word and orders near-duplicates
//! arbitrarily) and ordered here by:
//!
//! - term coverage: query words found in the name or brands (exact word,
//!   plural, prefix, words split differently such as "pompotes" for
//!   "Pom'Potes", or one typo), with a bonus when every word is found;
//! - name fit: how much of the name the query explains, ignoring words
//!   such as "cru" or "pulpe" that describe the plain form of a food;
//! - the name starting with the first query word, the query appearing as a
//!   phrase, and accented words typed with the same accents ("pâtes" is
//!   not "pâté");
//! - a table's default entry ("Fromage (aliment moyen)") for the food named;
//! - popularity: provider scans or how often the food was logged, on a log
//!   scale;
//! - whether the record has an energy value;
//! - a match only through a name in another language counts less.
//!
//! The weights were tuned on 100 common French product queries
//! (`tests/fixtures/off_ranking.json`) and 51 generic food queries
//! (`tests/fixtures/ciqual_ranking.json`, plus basic foods against the
//! bundled Ciqual 2025 table in `datasets::bundled`); halving or doubling any one of
//! the product weights changes top-1 accuracy by at most a few queries.

use crate::foods::normalize;

const W_COVERAGE: f64 = 10.0;
const W_ALL_TERMS: f64 = 3.0;
const W_NAME_FIT: f64 = 1.0;
const W_POPULARITY: f64 = 2.0;
const W_ENERGY: f64 = 4.0;
const W_LEADING: f64 = 1.0;
const W_GENERIC: f64 = 0.8;
const W_PHRASE: f64 = 1.0;
const W_ACCENTS: f64 = 1.0;
const W_OTHER_LANGUAGE: f64 = 3.0;

/// Words that carry no meaning on their own in product names.
const STOP_WORDS: &[&str] = &[
    "a", "au", "aux", "d", "de", "des", "du", "en", "et", "l", "la", "le", "les", "un", "une",
    "the", "of", "and", "y", "el", "los", "ou", "or", "o", "avec", "with", "con",
];

/// Preparation and part words that describe the plain form of a food.
const DESCRIPTORS: &[&str] = &[
    "cru",
    "crue",
    "crus",
    "crues",
    "cuit",
    "cuite",
    "cuits",
    "cuites",
    "pulpe",
    "peau",
    // Ciqual 2025 names the edible part: "Pomme, chair sans peau, crue".
    "chair",
    "sans",
    "flesh",
    "without",
    "skin",
    "viande",
    "nature",
    "standard",
    "courant",
    "courante",
    "classique",
    "non",
    "sale",
    "salee",
    "sales",
    "salees",
    "raw",
    "cooked",
    "meat",
    "plain",
    "crudo",
    "cruda",
    "cocido",
    "cocida",
    "carne",
];

/// Markers of a table's default entry for a kind of food.
const GENERIC_MARKERS: &[&[&str]] = &[
    &["aliment", "moyen"],
    // Ciqual's English names: "Potato, cooked (average food)", "Milk,
    // whole (average)".
    &["average", "food"],
    &["average"],
    &["sans", "precision"],
    &["not", "further", "specified"],
    &["nfs"],
];

/// Retailer private labels. Typed with a product ("picard pavé saumon"),
/// upstream text search ranks the common words far above the brand, so the
/// retailer's products may not be among the candidates at all; naming the
/// brand lets the provider run a second, brand-filtered search.
pub const RETAILER_BRANDS: &[&str] = &[
    "Auchan",
    "Bio Village",
    "Carrefour",
    "Carrefour Bio",
    "Casino",
    "Cora",
    "Éco+",
    "Franprix",
    "Intermarché",
    "Leader Price",
    "Leclerc",
    "Lidl",
    "Aldi",
    "Marque Repère",
    "Monoprix",
    "Netto",
    "Pâturages",
    "Picard",
    "Reflets de France",
    "Système U",
];

fn words(s: &str) -> Vec<String> {
    normalize(s)
        .split(' ')
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Lower-cased words with their accents, to tell "pâtes" from "pâté".
fn raw_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

fn is_stop(w: &str) -> bool {
    STOP_WORDS.contains(&w)
}

/// Drops a French/English plural ending: "pommes" ~ "pomme".
pub fn stem(w: &str) -> &str {
    if w.chars().count() > 3 && (w.ends_with('s') || w.ends_with('x')) {
        &w[..w.len() - 1]
    } else {
        w
    }
}

/// Edit distance of at most one (substitution, insertion or deletion).
fn within_one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let i = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    match a.len().cmp(&b.len()) {
        std::cmp::Ordering::Equal => i == a.len() || a[i + 1..] == b[i + 1..],
        std::cmp::Ordering::Greater => a[i + 1..] == b[i..],
        std::cmp::Ordering::Less => a[i..] == b[i + 1..],
    }
}

/// A normalized search query.
pub struct Query {
    words: Vec<String>,
    /// Words that must be matched; stop words only when nothing else is left.
    terms: Vec<String>,
    /// Words typed with accents, lower-cased but not folded.
    accented: Vec<String>,
}

impl Query {
    pub fn new(q: &str) -> Self {
        let words = words(q);
        let mut terms: Vec<String> = words.iter().filter(|w| !is_stop(w)).cloned().collect();
        if terms.is_empty() {
            terms = words.clone();
        }
        let accented = raw_words(q).into_iter().filter(|w| !w.is_ascii()).collect();
        Query {
            words,
            terms,
            accented,
        }
    }

    /// Word prefixes a stored food must contain to be a candidate: the
    /// meaningful terms without plural endings, so "oeufs" finds "Oeuf, cru".
    pub fn retrieval_terms(&self) -> Vec<&str> {
        self.terms.iter().take(6).map(|t| stem(t)).collect()
    }
}

/// What ranking needs to know about one result.
pub struct Candidate<'a> {
    /// The name shown, in the interface language where one exists.
    pub name: &'a str,
    /// Names in other languages; a match only through them counts less.
    pub other_names: Vec<&'a str>,
    pub brands: &'a [String],
    /// Provider scans, or how often the food was logged.
    pub popularity: u64,
    pub has_energy: bool,
}

/// How well one query term matches a product's words, from 0 to 1.
fn term_score(t: &str, words: &[String], compact: &str) -> f64 {
    let st = stem(t);
    let typo_ok = t.chars().count() >= 5;
    let mut best: f64 = 0.0;
    for w in words {
        if w == t {
            return 1.0;
        }
        let sw = stem(w);
        if sw == st {
            best = best.max(0.95);
        } else if t.len() >= 2 && w.starts_with(t) {
            best = best.max(0.75);
        } else if typo_ok && within_one_edit(st, sw) {
            best = best.max(0.5);
        }
    }
    // "kelloggs" in "Kellogg's", "pompotes" in "Pom'Potes".
    if best < 0.9 && t.chars().count() >= 3 && compact.contains(t) {
        best = 0.9;
    }
    best
}

struct Match {
    coverage: f64,
    all_terms: bool,
    /// Every term matched as a whole word or its plural.
    exact: bool,
}

fn match_terms(q: &Query, words: &[String]) -> Match {
    let compact: String = words.concat();
    let mut scores: Vec<f64> = q
        .terms
        .iter()
        .map(|t| term_score(t, words, &compact))
        .collect();
    // Two query words that only match together: "pom potes", "casseg rain".
    for i in 0..q.terms.len().saturating_sub(1) {
        if scores[i] >= 0.9 && scores[i + 1] >= 0.9 {
            continue;
        }
        let joined = format!("{}{}", q.terms[i], q.terms[i + 1]);
        let s = term_score(&joined, words, &compact);
        if s >= 0.9 {
            scores[i] = scores[i].max(s);
            scores[i + 1] = scores[i + 1].max(s);
        }
    }
    let n = scores.len().max(1) as f64;
    Match {
        coverage: scores.iter().sum::<f64>() / n,
        all_terms: scores.iter().all(|s| *s >= 0.75),
        exact: scores.iter().all(|s| *s >= 1.0),
    }
}

/// A name's words without the markers food composition tables use for
/// their default entry of a kind ("Fromage (aliment moyen)", "Brie, sans
/// précision", "Bread, NFS"), and whether one was present.
fn strip_generic(mut name: Vec<String>) -> (Vec<String>, bool) {
    let mut generic = false;
    for marker in GENERIC_MARKERS {
        let n = marker.len();
        if let Some(i) = (0..=name.len().saturating_sub(n))
            .find(|&i| name.len() >= n && name[i..i + n].iter().zip(*marker).all(|(a, b)| a == b))
        {
            name.drain(i..i + n);
            generic = true;
        }
    }
    (name, generic)
}

/// Share of the name's meaningful words that the query accounts for.
/// Preparation words ("Pomme, pulpe, crue") are left out: they describe
/// the plain form of a food rather than a different food.
fn name_fit(q: &Query, name: &[String]) -> f64 {
    let mut meaningful: Vec<&String> = name
        .iter()
        .filter(|w| !is_stop(w) && !DESCRIPTORS.contains(&w.as_str()))
        .collect();
    // "Lait fermenté, au lait entier" is no closer a match for repeating "lait".
    meaningful.sort();
    meaningful.dedup();
    if meaningful.is_empty() {
        return if name.is_empty() { 0.0 } else { 1.0 };
    }
    let explained = meaningful
        .iter()
        .filter(|w| {
            q.words.iter().any(|t| {
                stem(t) == stem(w) || (t.chars().count() >= 3 && w.starts_with(t.as_str()))
            })
        })
        .count();
    explained as f64 / meaningful.len() as f64
}

/// Whether the name starts with the first query term, as table names do
/// ("Orange, pulpe, crue" rather than "Nectar d'orange" for "orange").
fn leads(q: &Query, name: &[String]) -> bool {
    match (name.iter().find(|w| !is_stop(w)), q.terms.first()) {
        (Some(w), Some(t)) => stem(w) == stem(t),
        _ => false,
    }
}

/// Whether the query terms appear next to each other, in order, in the
/// name: "pain complet" in "Pain complet ou intégral", not in "Pain de mie,
/// complet".
fn has_phrase(q: &Query, name: &[String]) -> bool {
    if q.terms.len() < 2 {
        return false;
    }
    let name: Vec<&str> = name.iter().map(|w| stem(w)).collect();
    let words: Vec<&str> = q.words.iter().map(|t| stem(t)).collect();
    name.windows(words.len()).any(|w| w == words.as_slice())
}

/// Share of the accented query words found with the same accents.
fn accent_match(q: &Query, name: &str) -> f64 {
    if q.accented.is_empty() {
        return 0.0;
    }
    let raw = raw_words(name);
    let found = q.accented.iter().filter(|a| raw.contains(a)).count();
    found as f64 / q.accented.len() as f64
}

fn name_score(q: &Query, name: &str, brand_words: &[String]) -> (f64, bool) {
    let raw_name = name;
    let (name, generic) = strip_generic(words(name));
    let mut all = name.clone();
    all.extend_from_slice(brand_words);
    let m = match_terms(q, &all);
    let fit = name_fit(q, &name);
    let mut s = W_COVERAGE * m.coverage + W_NAME_FIT * fit + W_ACCENTS * accent_match(q, raw_name);
    if m.all_terms {
        s += W_ALL_TERMS;
    }
    // A table's default entry, when the query names exactly that food.
    if generic && m.exact {
        s += W_GENERIC * fit;
    }
    if leads(q, &name) {
        s += W_LEADING;
    }
    if has_phrase(q, &name) {
        s += W_PHRASE;
    }
    (s, m.all_terms)
}

fn brand_words(c: &Candidate) -> Vec<String> {
    c.brands.iter().flat_map(|b| words(b)).collect()
}

/// Relevance score; higher is better.
pub fn score(q: &Query, c: &Candidate) -> f64 {
    let brands = brand_words(c);
    let own = name_score(q, c.name, &brands).0;
    let other = c
        .other_names
        .iter()
        .map(|n| name_score(q, n, &brands).0 - W_OTHER_LANGUAGE)
        .fold(f64::MIN, f64::max);
    let mut s = own.max(other);
    if c.has_energy {
        s += W_ENERGY;
    }
    s + W_POPULARITY * (1.0 + c.popularity as f64).log10()
}

/// Whether every query term is found in one of the names or the brands.
pub fn matches_all(q: &Query, c: &Candidate) -> bool {
    let brands = brand_words(c);
    std::iter::once(&c.name)
        .chain(&c.other_names)
        .any(|n| name_score(q, n, &brands).1)
}

/// Orders items by descending score. Equal scores keep the given order.
pub fn sort<T>(q: &Query, items: Vec<T>, candidate: impl Fn(&T) -> Candidate<'_>) -> Vec<T> {
    let mut scored: Vec<(f64, T)> = items
        .into_iter()
        .map(|it| (score(q, &candidate(&it)), it))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, it)| it).collect()
}

/// A query split into a brand and the remaining words.
#[derive(Debug, PartialEq)]
pub struct BrandSplit {
    /// Brand names, as given, that the brand words can refer to.
    pub brands: Vec<String>,
    pub rest: Vec<String>,
}

/// Query words `[start, end)` covered by a brand's words, if any.
fn brand_span(brand: &[String], words: &[String]) -> Option<(usize, usize)> {
    if brand.is_empty() || brand.iter().all(|w| is_stop(w)) {
        return None;
    }
    let joined = brand.concat();
    (0..words.len()).find_map(|i| {
        if words[i..].starts_with(brand) {
            Some((i, i + brand.len()))
        } else if words[i] == joined && joined.chars().count() >= 3 {
            // "brets" for "Bret's".
            Some((i, i + 1))
        } else {
            None
        }
    })
}

/// Finds the longest run of query words naming one of `brands` ("carrefour
/// bio" over "carrefour"). Returns every brand spelled that way, and the
/// other words, or `None` when no brand is named or nothing else is left.
pub fn split_brand<'a>(q: &Query, brands: impl IntoIterator<Item = &'a str>) -> Option<BrandSplit> {
    let mut best: Option<((usize, usize), Vec<String>)> = None;
    for b in brands {
        let Some(span) = brand_span(&words(b), &q.words) else {
            continue;
        };
        let longer = |(s, e): (usize, usize), (bs, be): (usize, usize)| {
            (e - s, std::cmp::Reverse(s)) > (be - bs, std::cmp::Reverse(bs))
        };
        match &mut best {
            Some((bspan, names)) if *bspan == span => {
                if !names.iter().any(|n| n == b) {
                    names.push(b.to_string());
                }
            }
            Some((bspan, _)) if !longer(span, *bspan) => {}
            _ => best = Some((span, vec![b.to_string()])),
        }
    }
    let ((start, end), brands) = best?;
    let rest: Vec<String> = q
        .words
        .iter()
        .enumerate()
        .filter(|(i, _)| *i < start || *i >= end)
        .map(|(_, w)| w.clone())
        .collect();
    (!rest.is_empty()).then_some(BrandSplit { brands, rest })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand<'a>(name: &'a str, brands: &'a [String], scans: u64) -> Candidate<'a> {
        Candidate {
            name,
            other_names: Vec::new(),
            brands,
            popularity: scans,
            has_energy: true,
        }
    }

    fn brands(b: &[&str]) -> Vec<String> {
        b.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_edit() {
        assert!(within_one_edit("tagada", "tagadda"));
        assert!(within_one_edit("tagada", "tagado"));
        assert!(within_one_edit("tagada", "tagad"));
        assert!(!within_one_edit("tagada", "tadaga"));
        assert!(within_one_edit("", "a"));
    }

    #[test]
    fn term_matches_plural_prefix_joined_and_typo() {
        let q = Query::new("pompotes pomme");
        let b = brands(&["Pom'Potes"]);
        assert!(matches_all(&q, &cand("Compotes pommes", &b, 0)));
        let q = Query::new("corn flakes kelloggs");
        let b = brands(&["Kellogg's"]);
        assert!(matches_all(&q, &cand("Corn Flakes", &b, 0)));
        let q = Query::new("haricots verts casseg rain");
        let b = brands(&["Cassegrain"]);
        assert!(matches_all(&q, &cand("Haricots verts", &b, 0)));
        assert!(!matches_all(&q, &cand("Haricots verts", &[], 0)));
    }

    #[test]
    fn brand_word_outranks_partial_match() {
        let q = Query::new("camembert président");
        let president = brands(&["Président"]);
        let other = brands(&["Lepetit"]);
        let full = score(&q, &cand("Camembert", &president, 1));
        let partial = score(&q, &cand("Camembert", &other, 300));
        assert!(full > partial);
    }

    #[test]
    fn popular_duplicate_and_energy_value_win() {
        let q = Query::new("activia nature");
        let b = brands(&["Activia"]);
        assert!(
            score(&q, &cand("Activia nature", &b, 40)) > score(&q, &cand("Activia nature", &b, 1))
        );
        let mut missing = cand("Activia nature", &b, 40);
        missing.has_energy = false;
        assert!(score(&q, &cand("Activia nature", &b, 1)) > score(&q, &missing));
    }

    #[test]
    fn sort_keeps_upstream_order_on_ties() {
        let q = Query::new("kiri");
        let b = brands(&["Kiri"]);
        let items = vec![("first", 1), ("second", 1), ("popular", 50)];
        let sorted = sort(&q, items, |(_, s)| cand("Kiri", &b, *s));
        assert_eq!(
            sorted.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            ["popular", "first", "second"]
        );
    }

    #[test]
    fn splits_brand_from_query() {
        let q = Query::new("picard pavé saumon");
        assert_eq!(
            split_brand(&q, RETAILER_BRANDS.iter().copied()),
            Some(BrandSplit {
                brands: brands(&["Picard"]),
                rest: brands(&["pave", "saumon"]),
            })
        );
        // The longest brand wins and every spelling of it is kept.
        let q = Query::new("eco plus coquillettes");
        let split = split_brand(&q, ["Eco Plus", "Éco+", "Eco"]).unwrap();
        assert_eq!(split.brands, brands(&["Eco Plus", "Éco+"]));
        assert_eq!(split.rest, brands(&["coquillettes"]));
        let q = Query::new("carrefour bio muesli");
        assert_eq!(
            split_brand(&q, RETAILER_BRANDS.iter().copied())
                .unwrap()
                .brands,
            brands(&["Carrefour Bio"])
        );
        // Joined spelling.
        let q = Query::new("chips brets");
        assert_eq!(
            split_brand(&q, ["Bret's"]).unwrap().brands,
            brands(&["Bret's"])
        );
        // A brand alone leaves nothing to search for.
        assert_eq!(split_brand(&Query::new("picard"), ["Picard"]), None);
        assert_eq!(split_brand(&Query::new("saumon"), ["Picard"]), None);
    }
}
