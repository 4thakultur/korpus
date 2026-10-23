//! ANSES-CIQUAL (France), from the XML zip download.
//!
//! `alim_*.xml` lists foods with French and English names; `compo_*.xml`
//! gives one value per food and constituent, per 100 g. The files are
//! Windows-1252 and not well-formed XML: values such as `< 2,2` contain a
//! raw `<`. Records are therefore read with a small tolerant scanner that
//! only looks for the known field tags.

use std::collections::HashMap;

use super::archive::{UploadFile, Zip, decode_text};
use super::value::{FoodBuilder, Value};
use super::{DatasetFood, ImportError};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient};

// Constituent codes (`const_*.xml`).
const KCAL: &str = "328"; // Energy, Regulation EU No 1169/2011 (kcal/100 g)
const KJ: &str = "327";
const PROTEIN: &str = "25000";
const CARBS: &str = "31000";
const FAT: &str = "40000";
const FIBER: &str = "34100";
const SUGARS: &str = "32000";
const SAT_FAT: &str = "40302";
const SALT: &str = "10004";
const SODIUM_MG: &str = "10110";
const WANTED: &[&str] = &[
    KCAL, KJ, PROTEIN, CARBS, FAT, FIBER, SUGARS, SAT_FAT, SALT, SODIUM_MG,
];

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let file = &files[0];
    let mut zip = Zip::open(file)?;
    let alim = decode_text(&zip.read("alim_….xml", |n| {
        n.starts_with("alim_") && !n.starts_with("alim_grp") && n.ends_with(".xml")
    })?);
    let compo = decode_text(&zip.read("compo_….xml", |n| {
        n.starts_with("compo_") && n.ends_with(".xml")
    })?);
    parse_xml(&alim, &compo)
}

fn parse_xml(alim: &str, compo: &str) -> Result<Vec<DatasetFood>, ImportError> {
    let mut values: HashMap<String, HashMap<String, String>> = HashMap::new();
    for rec in records(compo, "COMPO") {
        let (Some(food), Some(constituent)) = (field(rec, "alim_code"), field(rec, "const_code"))
        else {
            continue;
        };
        if WANTED.contains(&constituent.as_str()) {
            values
                .entry(food)
                .or_default()
                .insert(constituent, field(rec, "teneur").unwrap_or_default());
        }
    }

    let mut foods = Vec::new();
    let mut found = false;
    for rec in records(alim, "ALIM") {
        found = true;
        let Some(code) = field(rec, "alim_code") else {
            continue;
        };
        let empty = HashMap::new();
        let v = values.get(&code).unwrap_or(&empty);
        let get = |c: &str| v.get(c).map(|s| Value::parse(s)).unwrap_or(Value::Missing);
        let mut b = FoodBuilder::new(code.clone(), Basis::Per100g);
        b.name(Lang::Fr, &field(rec, "alim_nom_fr").unwrap_or_default());
        b.name(Lang::En, &field(rec, "alim_nom_eng").unwrap_or_default());
        b.set(Nutrient::Energy, get(KCAL));
        b.energy_kj(get(KJ));
        b.set(Nutrient::Protein, get(PROTEIN));
        b.set(Nutrient::Carbs, get(CARBS));
        b.set(Nutrient::Fat, get(FAT));
        b.set(Nutrient::Fiber, get(FIBER));
        b.set(Nutrient::Sugars, get(SUGARS));
        b.set(Nutrient::SatFat, get(SAT_FAT));
        b.set(Nutrient::Salt, get(SALT));
        b.sodium_mg(get(SODIUM_MG));
        foods.extend(b.build());
    }
    if !found {
        return Err(ImportError::Format {
            file: "alim_….xml".into(),
            detail: "no ALIM records".into(),
        });
    }
    Ok(foods)
}

/// The text of each `<tag>…</tag>` record.
fn records<'a>(text: &'a str, tag: &str) -> impl Iterator<Item = &'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut rest = text;
    std::iter::from_fn(move || {
        let start = rest.find(&open)? + open.len();
        let len = rest[start..].find(&close)?;
        let rec = &rest[start..start + len];
        rest = &rest[start + len + close.len()..];
        Some(rec)
    })
}

/// The trimmed, unescaped text of `<name>…</name>` within a record.
fn field(rec: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = rec.find(&open)? + open.len();
    let len = rec[start..].find(&format!("</{name}>"))?;
    let text = rec[start..start + len].trim();
    Some(
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::archive::tests::zip_of;

    // Records copied from the 2020-07-07 release, including its raw "<"
    // values and its "-" for values not published.
    const ALIM: &str = "<?xml version=\"1.0\" encoding=\"windows-1252\" ?>\r\n<TABLE>\r\n\
   <ALIM>\r\n      <alim_code> 13039 </alim_code>\r\n      <alim_nom_fr> Pomme, pulpe et peau, crue </alim_nom_fr>\r\n\
      <ALIM_NOM_INDEX_FR> Pomme, pulpe et peau, crue </ALIM_NOM_INDEX_FR>\r\n\
      <alim_nom_eng> Apple, pulp and peel, raw </alim_nom_eng>\r\n   </ALIM>\r\n\
   <ALIM>\r\n      <alim_code> 1022 </alim_code>\r\n      <alim_nom_fr> Cocktail type punch, 16% alcool </alim_nom_fr>\r\n\
      <alim_nom_eng> Cocktail, punch type, 16% alcohol </alim_nom_eng>\r\n   </ALIM>\r\n\
   <ALIM>\r\n      <alim_code> 1000 </alim_code>\r\n      <alim_nom_fr> Pastis </alim_nom_fr>\r\n\
      <alim_nom_eng> Pastis (anise-flavoured spirit) </alim_nom_eng>\r\n   </ALIM>\r\n</TABLE>\r\n";
    const COMPO: &str = "<TABLE>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 327 </const_code>\r\n      <teneur> - </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 328 </const_code>\r\n      <teneur> - </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 10004 </const_code>\r\n      <teneur> 0,0037 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 10110 </const_code>\r\n      <teneur> 1,5 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 25000 </const_code>\r\n      <teneur> 0,25 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 31000 </const_code>\r\n      <teneur> 11,6 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 32000 </const_code>\r\n      <teneur> 9,35 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 34100 </const_code>\r\n      <teneur> 1,4 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 40000 </const_code>\r\n      <teneur> 0,25 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 13039 </alim_code>\r\n      <const_code> 40302 </const_code>\r\n      <teneur> 0,052 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 327 </const_code>\r\n      <teneur> - </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 328 </const_code>\r\n      <teneur> - </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 10004 </const_code>\r\n      <teneur> 0,042 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 10110 </const_code>\r\n      <teneur> 16,7 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 25000 </const_code>\r\n      <teneur> 0,6 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 31000 </const_code>\r\n      <teneur> 17,8 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 32000 </const_code>\r\n      <teneur> 17,8 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 34100 </const_code>\r\n      <teneur> < 0,5 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 40000 </const_code>\r\n      <teneur> < 0,6 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1022 </alim_code>\r\n      <const_code> 40302 </const_code>\r\n      <teneur> < 0,00024 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 327 </const_code>\r\n      <teneur> 1140 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 328 </const_code>\r\n      <teneur> 274 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 10004 </const_code>\r\n      <teneur> 0 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 25000 </const_code>\r\n      <teneur> 0 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 31000 </const_code>\r\n      <teneur> 2,86 </teneur>\r\n      <min missing=\" \" />\r\n   </COMPO>\r\n\
</TABLE>\r\n";

    #[test]
    fn reads_names_values_and_markers() {
        let foods = parse_xml(ALIM, COMPO).unwrap();
        let apple = &foods[0];
        assert_eq!(apple.code, "13039");
        assert_eq!(
            apple.names,
            [
                (Lang::Fr, "Pomme, pulpe et peau, crue".to_string()),
                (Lang::En, "Apple, pulp and peel, raw".to_string()),
            ]
        );
        assert_eq!(apple.nutrients.energy_kcal, None, "\"-\" is not published");
        assert_eq!(apple.nutrients.carbs_g, Some(11.6));
        assert_eq!(apple.nutrients.sat_fat_g, Some(0.052));
        assert_eq!(apple.nutrients.salt_g, Some(0.0037), "published salt wins");
        assert!(!apple.salt_from_sodium);
        assert!(apple.trace.is_empty());

        let punch = &foods[1];
        assert_eq!(punch.nutrients.fat_g, Some(0.0));
        assert_eq!(punch.nutrients.protein_g, Some(0.6));
        assert_eq!(
            punch.trace,
            [Nutrient::Fat, Nutrient::Fiber, Nutrient::SatFat],
            "below the detection limit"
        );

        let pastis = &foods[2];
        assert_eq!(
            pastis.nutrients.energy_kcal,
            Some(274.0),
            "kcal is preferred"
        );
        assert!(!pastis.energy_from_kj);
        assert_eq!(pastis.nutrients.carbs_g, Some(2.86));
    }

    #[test]
    fn uses_kj_when_kcal_is_missing() {
        let compo = "<COMPO><alim_code>1000</alim_code><const_code>327</const_code><teneur> 1140 </teneur></COMPO>";
        let foods = parse_xml(ALIM, compo).unwrap();
        let pastis = &foods[2];
        assert!(pastis.energy_from_kj);
        assert!((pastis.nutrients.energy_kcal.unwrap() - 1140.0 / 4.184).abs() < 1e-9);
    }

    #[test]
    fn reads_the_zip_and_skips_the_group_file() {
        let z = zip_of(&[
            ("XML_2020_07_07/alim_grp_2020_07_07.xml", b"<TABLE></TABLE>"),
            ("XML_2020_07_07/alim_2020_07_07.xml", ALIM.as_bytes()),
            ("XML_2020_07_07/compo_2020_07_07.xml", COMPO.as_bytes()),
        ]);
        let foods = parse(&[z]).unwrap();
        assert_eq!(foods.len(), 3);
        let z = zip_of(&[
            ("alim_2020.xml", b"<TABLE></TABLE>"),
            ("compo_2020.xml", b""),
        ]);
        assert!(matches!(parse(&[z]), Err(ImportError::Format { .. })));
    }

    #[test]
    fn decodes_accented_names() {
        let alim =
            "<ALIM><alim_code>1</alim_code><alim_nom_fr>Crème brûlée &amp; co</alim_nom_fr></ALIM>";
        let bytes = encoding_rs::WINDOWS_1252.encode(alim).0.into_owned();
        let text = decode_text(&bytes);
        let foods = parse_xml(&text, "").unwrap();
        assert_eq!(foods[0].names[0].1, "Crème brûlée & co");
    }
}
