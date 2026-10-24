//! ANSES-CIQUAL (France), from the XML download: the zip of the 2020
//! release, or the separate `alim_*.xml` and `compo_*.xml` files of later
//! releases.
//!
//! `alim_*.xml` lists foods with French and English names; `compo_*.xml`
//! gives one value per food and constituent, per 100 g. The 2020 files are
//! Windows-1252 and not well-formed XML: values such as `< 2,2` contain a
//! raw `<`. Records are therefore read with a small tolerant scanner that
//! only looks for the known field tags.
//!
//! Korpus also ships an extract of one release (see `bundled/ciqual/`):
//! the same records as tab-separated text, with each value exactly as
//! published, read by [`parse_extract`] into the same foods.

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

const ALIM_LABEL: &str = "alim_….xml";
const COMPO_LABEL: &str = "compo_….xml";

fn is_alim(name: &str) -> bool {
    name.starts_with("alim_") && !name.starts_with("alim_grp") && name.ends_with(".xml")
}

fn is_compo(name: &str) -> bool {
    name.starts_with("compo_") && name.ends_with(".xml")
}

/// One food as published: its names and the raw value of each wanted
/// constituent, by constituent code.
struct Record {
    code: String,
    name_fr: String,
    name_en: String,
    values: HashMap<String, String>,
}

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let (alim, compo) = read_files(files)?;
    Ok(build(read_xml(&alim, &compo)?))
}

/// The text of `alim_*.xml` and `compo_*.xml`, uploaded directly or inside
/// a zip.
fn read_files(files: &[UploadFile]) -> Result<(String, String), ImportError> {
    let base = |f: &UploadFile| {
        let n = f.name.rsplit(['/', '\\']).next().unwrap_or(&f.name);
        n.to_ascii_lowercase()
    };
    let mut alim = files
        .iter()
        .find(|f| is_alim(&base(f)))
        .map(|f| decode_text(&f.bytes));
    let mut compo = files
        .iter()
        .find(|f| is_compo(&base(f)))
        .map(|f| decode_text(&f.bytes));
    for f in files.iter().filter(|f| base(f).ends_with(".zip")) {
        let mut zip = Zip::open(f)?;
        if alim.is_none() {
            alim = Some(decode_text(&zip.read(ALIM_LABEL, is_alim)?));
        }
        if compo.is_none() {
            compo = Some(decode_text(&zip.read(COMPO_LABEL, is_compo)?));
        }
    }
    match (alim, compo) {
        (Some(a), Some(c)) => Ok((a, c)),
        (None, _) => Err(ImportError::MissingFile(ALIM_LABEL)),
        (_, None) => Err(ImportError::MissingFile(COMPO_LABEL)),
    }
}

fn read_xml(alim: &str, compo: &str) -> Result<Vec<Record>, ImportError> {
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

    let mut out = Vec::new();
    let mut found = false;
    for rec in records(alim, "ALIM") {
        found = true;
        let Some(code) = field(rec, "alim_code") else {
            continue;
        };
        out.push(Record {
            values: values.remove(&code).unwrap_or_default(),
            name_fr: field(rec, "alim_nom_fr").unwrap_or_default(),
            name_en: field(rec, "alim_nom_eng").unwrap_or_default(),
            code,
        });
    }
    if !found {
        return Err(ImportError::Format {
            file: ALIM_LABEL.into(),
            detail: "no ALIM records".into(),
        });
    }
    Ok(out)
}

fn build(records: Vec<Record>) -> Vec<DatasetFood> {
    let mut foods = Vec::new();
    for r in records {
        let get = |c: &str| {
            r.values
                .get(c)
                .map(|s| Value::parse(s))
                .unwrap_or(Value::Missing)
        };
        let mut b = FoodBuilder::new(r.code.clone(), Basis::Per100g);
        b.name(Lang::Fr, &r.name_fr);
        b.name(Lang::En, &r.name_en);
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
    foods
}

const EXTRACT_NAMES: [&str; 3] = ["alim_code", "alim_nom_fr", "alim_nom_eng"];

/// The records Korpus reads from the publisher's files, as tab-separated
/// text: a header of field names and constituent codes, then one line per
/// food with every value as published (trimmed and unescaped).
pub fn extract(files: &[UploadFile]) -> Result<String, ImportError> {
    let (alim, compo) = read_files(files)?;
    let mut out = EXTRACT_NAMES
        .iter()
        .chain(WANTED)
        .copied()
        .collect::<Vec<_>>()
        .join("\t");
    out.push('\n');
    for r in read_xml(&alim, &compo)? {
        let mut cells = vec![r.code.as_str(), &r.name_fr, &r.name_en];
        cells.extend(
            WANTED
                .iter()
                .map(|c| r.values.get(*c).map(String::as_str).unwrap_or("")),
        );
        if let Some(bad) = cells.iter().find(|c| c.contains(['\t', '\n', '\r'])) {
            return Err(ImportError::Format {
                file: format!("food {}", r.code),
                detail: format!("tab or line break in {bad:?}"),
            });
        }
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    Ok(out)
}

/// Reads text written by [`extract`].
pub fn parse_extract(text: &str) -> Result<Vec<DatasetFood>, ImportError> {
    let format = |detail: &str| ImportError::Format {
        file: "Ciqual extract".into(),
        detail: detail.into(),
    };
    let mut lines = text.lines();
    let header: Vec<&str> = lines
        .next()
        .ok_or_else(|| format("empty"))?
        .split('\t')
        .collect();
    let column = |name: &str| header.iter().position(|h| *h == name);
    let [code, fr, en] = EXTRACT_NAMES.map(column);
    let (Some(code), Some(fr), Some(en)) = (code, fr, en) else {
        return Err(format("missing name columns"));
    };
    let constituents: Vec<(usize, &str)> = WANTED
        .iter()
        .filter_map(|c| Some((column(c)?, *c)))
        .collect();
    let mut records = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != header.len() {
            return Err(format("wrong number of columns"));
        }
        records.push(Record {
            code: cells[code].to_string(),
            name_fr: cells[fr].to_string(),
            name_en: cells[en].to_string(),
            values: constituents
                .iter()
                .map(|(i, c)| (c.to_string(), cells[*i].to_string()))
                .collect(),
        });
    }
    Ok(build(records))
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

/// The unescaped text of `<name>…</name>` within a record, with runs of
/// whitespace (including the occasional line break inside a name) as one
/// space.
fn field(rec: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = rec.find(&open)? + open.len();
    let len = rec[start..].find(&format!("</{name}>"))?;
    let text = rec[start..start + len]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
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
        let foods = build(read_xml(ALIM, COMPO).unwrap());
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
        let foods = build(read_xml(ALIM, compo).unwrap());
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
        let foods = build(read_xml(&text, "").unwrap());
        assert_eq!(foods[0].names[0].1, "Crème brûlée & co");
    }

    fn upload(name: &str, text: &str) -> UploadFile {
        UploadFile {
            name: name.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    // The 2025-11-03 release: separate UTF-8 files with a byte order mark,
    // "<" escaped, and extra fields.
    const ALIM_2025: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\" ?>\r\n<TABLE>\r\n\
   <ALIM>\r\n      <alim_code> 1000 </alim_code>\r\n      <alim_nom_fr> Pastis </alim_nom_fr>\r\n\
      <alim_nom_eng> Pastis (anise-flavoured\r\nspirit) </alim_nom_eng>\r\n      <alim_nom_sci missing=\" \" />\r\n\
      <alim_grp_code> 06 </alim_grp_code>\r\n      <facteur_Jones> 6.25 </facteur_Jones>\r\n   </ALIM>\r\n</TABLE>\r\n";
    const COMPO_2025: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\" ?>\r\n<TABLE>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 328 </const_code>\r\n      <teneur> 274 </teneur>\r\n\
      <min missing=\" \" />\r\n      <code_confiance> D </code_confiance>\r\n      <source_code> 444 </source_code>\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 40000 </const_code>\r\n      <teneur> &lt; 0,2 </teneur>\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 32000 </const_code>\r\n      <teneur> traces </teneur>\r\n   </COMPO>\r\n\
   <COMPO>\r\n      <alim_code> 1000 </alim_code>\r\n      <const_code> 25000 </const_code>\r\n      <teneur> - </teneur>\r\n   </COMPO>\r\n\
</TABLE>\r\n";

    #[test]
    fn reads_separate_files_of_later_releases() {
        let files = [
            upload("alim_grp_2025_11_03.xml", "<TABLE></TABLE>"),
            upload("C:\\Downloads\\alim_2025_11_03.xml", ALIM_2025),
            upload("compo_2025_11_03.xml", COMPO_2025),
        ];
        let foods = parse(&files).unwrap();
        assert_eq!(foods.len(), 1);
        let pastis = &foods[0];
        assert_eq!(pastis.code, "1000");
        assert_eq!(pastis.names[0], (Lang::Fr, "Pastis".to_string()));
        assert_eq!(
            pastis.names[1],
            (Lang::En, "Pastis (anise-flavoured spirit)".to_string()),
            "a line break inside a name is a space"
        );
        assert_eq!(pastis.nutrients.energy_kcal, Some(274.0));
        assert_eq!(pastis.nutrients.fat_g, Some(0.0));
        assert_eq!(pastis.nutrients.protein_g, None);
        assert_eq!(pastis.trace, [Nutrient::Fat, Nutrient::Sugars]);

        assert!(matches!(
            parse(&files[1..2]),
            Err(ImportError::MissingFile(COMPO_LABEL))
        ));
        assert!(matches!(
            parse(&files[2..]),
            Err(ImportError::MissingFile(ALIM_LABEL))
        ));
    }

    #[test]
    fn extract_reads_back_as_the_same_foods() {
        let z = zip_of(&[
            ("alim_2020_07_07.xml", ALIM.as_bytes()),
            ("compo_2020_07_07.xml", COMPO.as_bytes()),
        ]);
        let text = extract(std::slice::from_ref(&z)).unwrap();
        assert!(text.starts_with("alim_code\talim_nom_fr\talim_nom_eng\t328\t"));
        assert!(text.contains("\t< 0,5\t"), "values are kept as published");
        assert_eq!(parse_extract(&text).unwrap(), parse(&[z]).unwrap());

        // Columns are found by name, so their order may change.
        let swapped = "alim_nom_eng\t328\talim_code\talim_nom_fr\nPastis\t274\t1000\tPastis\n";
        let foods = parse_extract(swapped).unwrap();
        assert_eq!(foods[0].code, "1000");
        assert_eq!(foods[0].nutrients.energy_kcal, Some(274.0));
        assert!(matches!(
            parse_extract("alim_code\talim_nom_fr\talim_nom_eng\n1\tx\n"),
            Err(ImportError::Format { .. })
        ));
    }
}
