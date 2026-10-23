//! USDA FoodData Central, from the CSV zip downloads of Foundation Foods,
//! SR Legacy and FNDDS (survey) foods.
//!
//! Each zip has `food.csv` (fdc_id, data_type, description) and
//! `food_nutrient.csv` (fdc_id, nutrient_id, amount) with amounts per 100 g.
//! Foundation zips also list the samples behind each food; only rows whose
//! data type is a food are kept. Branded foods are not imported: they are
//! a multi-gigabyte download and packaged products are better served by
//! barcode lookup.

use std::collections::{HashMap, HashSet};

use super::archive::{UploadFile, Zip, decode_text};
use super::csv::{Table, field};
use super::value::{FoodBuilder, Value};
use super::{DatasetFood, ImportError};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient};

const FOOD_TYPES: &[&str] = &["foundation_food", "sr_legacy_food", "survey_fndds_food"];

// FoodData Central nutrient ids.
const KCAL: u32 = 1008;
const KCAL_ATWATER_SPECIFIC: u32 = 2048;
const KCAL_ATWATER_GENERAL: u32 = 2047;
const KJ: u32 = 1062;
const PROTEIN: u32 = 1003;
const FAT: u32 = 1004;
const FAT_NLEA: u32 = 1085;
const CARBS_BY_DIFFERENCE: u32 = 1005;
const CARBS_BY_SUMMATION: u32 = 1050;
const FIBER: u32 = 1079;
const SUGARS: u32 = 2000;
const SUGARS_NLEA: u32 = 1063;
const SAT_FAT: u32 = 1258;
const SODIUM_MG: u32 = 1093;

const WANTED: &[u32] = &[
    KCAL,
    KCAL_ATWATER_SPECIFIC,
    KCAL_ATWATER_GENERAL,
    KJ,
    PROTEIN,
    FAT,
    FAT_NLEA,
    CARBS_BY_DIFFERENCE,
    CARBS_BY_SUMMATION,
    FIBER,
    SUGARS,
    SUGARS_NLEA,
    SAT_FAT,
    SODIUM_MG,
];

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let mut foods: Vec<DatasetFood> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for file in files {
        let mut zip = Zip::open(file)?;
        let food_csv = decode_text(&zip.read_named("food.csv")?);
        let nutrient_csv = decode_text(&zip.read_named("food_nutrient.csv")?);
        for f in parse_tables(&food_csv, &nutrient_csv)? {
            // The same food may appear in two uploads; keep the first.
            if seen.insert(f.code.clone()) {
                foods.push(f);
            }
        }
    }
    Ok(foods)
}

fn parse_tables(food_csv: &str, nutrient_csv: &str) -> Result<Vec<DatasetFood>, ImportError> {
    let mut table = Table::new(food_csv, ',', "food.csv")?;
    let (id_col, type_col, desc_col) = (
        table.column("fdc_id")?,
        table.column("data_type")?,
        table.column("description")?,
    );
    let mut names: Vec<(String, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for row in table.rows.by_ref() {
        if FOOD_TYPES.contains(&field(&row, type_col)) {
            let id = field(&row, id_col).to_string();
            index.insert(id.clone(), names.len());
            names.push((id, field(&row, desc_col).to_string()));
        }
    }

    let mut values: Vec<HashMap<u32, f64>> = vec![HashMap::new(); names.len()];
    let mut table = Table::new(nutrient_csv, ',', "food_nutrient.csv")?;
    let (id_col, nutrient_col, amount_col) = (
        table.column("fdc_id")?,
        table.column("nutrient_id")?,
        table.column("amount")?,
    );
    for row in table.rows.by_ref() {
        let Some(&i) = index.get(field(&row, id_col)) else {
            continue;
        };
        let Ok(nutrient) = field(&row, nutrient_col).parse::<u32>() else {
            continue;
        };
        if WANTED.contains(&nutrient)
            && let Value::Number(v) = Value::parse(field(&row, amount_col))
        {
            values[i].entry(nutrient).or_insert(v);
        }
    }

    Ok(names
        .into_iter()
        .zip(values)
        .filter_map(|((id, name), v)| food(id, &name, &v))
        .collect())
}

fn food(id: String, name: &str, v: &HashMap<u32, f64>) -> Option<DatasetFood> {
    let get = |n: u32| Value::from(v.get(&n).copied());
    let mut b = FoodBuilder::new(id, Basis::Per100g);
    b.name(Lang::En, name);
    for kcal in [KCAL, KCAL_ATWATER_SPECIFIC, KCAL_ATWATER_GENERAL] {
        b.set(Nutrient::Energy, get(kcal));
    }
    b.energy_kj(get(KJ));
    b.set(Nutrient::Protein, get(PROTEIN));
    b.set(Nutrient::Fat, get(FAT));
    b.set(Nutrient::Fat, get(FAT_NLEA));
    // "By difference" is the usual US figure and includes fibre; the sum of
    // sugars and starch does not.
    if b.set(Nutrient::Carbs, get(CARBS_BY_DIFFERENCE)) {
        b.carbs_include_fiber();
    } else {
        b.set(Nutrient::Carbs, get(CARBS_BY_SUMMATION));
    }
    b.set(Nutrient::Fiber, get(FIBER));
    b.set(Nutrient::Sugars, get(SUGARS));
    b.set(Nutrient::Sugars, get(SUGARS_NLEA));
    b.set(Nutrient::SatFat, get(SAT_FAT));
    b.sodium_mg(get(SODIUM_MG));
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::archive::tests::zip_of;

    // Rows copied from the SR Legacy (2018-04) and Foundation (2026-04-30)
    // downloads, trimmed to the columns and nutrients used here.
    const FOOD: &str = "\"fdc_id\",\"data_type\",\"description\",\"food_category_id\",\"publication_date\"\n\
\"170899\",\"sr_legacy_food\",\"Cheese, cheddar, sharp, sliced\",\"1\",\"2019-04-01\"\n\
\"319875\",\"market_acquisition\",\"HUMMUS, SABRA CLASSIC\",\"16\",\"2019-04-01\"\n\
\"321358\",\"foundation_food\",\"Hummus, commercial\",\"16\",\"2019-04-01\"\n";
    const NUTRIENTS: &str = "\"id\",\"fdc_id\",\"nutrient_id\",\"amount\",\"data_points\"\n\
\"1563749\",\"170899\",\"1003\",\"24.25\",\"18\"\n\
\"1563756\",\"170899\",\"1079\",\"0\",\"0\"\n\
\"1563760\",\"170899\",\"1093\",\"644\",\"18\"\n\
\"1563795\",\"170899\",\"1258\",\"19.368\",\"0\"\n\
\"1563819\",\"170899\",\"1004\",\"33.82\",\"18\"\n\
\"1563820\",\"170899\",\"1005\",\"2.13\",\"0\"\n\
\"1563821\",\"170899\",\"1008\",\"410\",\"0\"\n\
\"1563828\",\"170899\",\"2000\",\"0.27\",\"3\"\n\
\"2219740\",\"321358\",\"1003\",\"7.35\",\"\"\n\
\"2219751\",\"321358\",\"1093\",\"438.0\",\"11\"\n\
\"2219786\",\"321358\",\"1004\",\"17.1\",\"11\"\n\
\"2219814\",\"321358\",\"1050\",\"13.9\",\"\"\n\
\"13338510\",\"321358\",\"2047\",\"243\",\"\"\n\
\"13338412\",\"321358\",\"2048\",\"229\",\"\"\n";

    #[test]
    fn reads_food_rows_and_nutrients() {
        let foods = parse_tables(FOOD, NUTRIENTS).unwrap();
        assert_eq!(foods.len(), 2, "samples are skipped");

        let cheddar = &foods[0];
        assert_eq!(cheddar.code, "170899");
        assert_eq!(
            cheddar.names,
            [(Lang::En, "Cheese, cheddar, sharp, sliced".to_string())]
        );
        assert_eq!(cheddar.nutrients.energy_kcal, Some(410.0));
        assert_eq!(cheddar.nutrients.protein_g, Some(24.25));
        assert_eq!(cheddar.nutrients.carbs_g, Some(2.13));
        assert!(cheddar.carbs_include_fiber);
        assert_eq!(cheddar.nutrients.fiber_g, Some(0.0));
        assert!(cheddar.salt_from_sodium);
        assert!((cheddar.nutrients.salt_g.unwrap() - 1.61).abs() < 1e-9);

        // This Foundation food has no 1008 or 1005 rows.
        let hummus = &foods[1];
        assert_eq!(
            hummus.nutrients.energy_kcal,
            Some(229.0),
            "specific factors first"
        );
        assert_eq!(hummus.nutrients.carbs_g, Some(13.9));
        assert!(!hummus.carbs_include_fiber, "summation excludes fibre");
        assert_eq!(hummus.nutrients.sugars_g, None, "missing stays missing");
    }

    #[test]
    fn merges_several_zips_without_duplicates() {
        let z = zip_of(&[
            ("FoodData/food.csv", FOOD.as_bytes()),
            ("FoodData/food_nutrient.csv", NUTRIENTS.as_bytes()),
        ]);
        let again = zip_of(&[
            ("food.csv", FOOD.as_bytes()),
            ("food_nutrient.csv", NUTRIENTS.as_bytes()),
        ]);
        assert_eq!(parse(&[z, again]).unwrap().len(), 2);
    }

    #[test]
    fn wrong_upload_is_reported() {
        let z = zip_of(&[("food.csv", FOOD.as_bytes())]);
        assert_eq!(
            parse(&[z]),
            Err(ImportError::MissingFile("food_nutrient.csv"))
        );
        let z = zip_of(&[("food.csv", b"x,y\n1,2\n"), ("food_nutrient.csv", b"a\n")]);
        assert!(matches!(parse(&[z]), Err(ImportError::Format { .. })));
    }
}
