//! Canadian Nutrient File (Health Canada), from the CSV zip download.
//!
//! `FOOD NAME.csv` has English and French names; `NUTRIENT AMOUNT.csv` one
//! value per food and nutrient, per 100 g. Files are Latin-1.

use std::collections::HashMap;

use super::archive::{UploadFile, Zip, decode_text};
use super::csv::{Table, field};
use super::value::{FoodBuilder, Value};
use super::{DatasetFood, ImportError};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient};

// `NUTRIENT NAME.csv` ids.
const KCAL: u32 = 208;
const KJ: u32 = 268;
const PROTEIN: u32 = 203;
const FAT: u32 = 204;
const CARBS_BY_DIFFERENCE: u32 = 205;
const FIBER: u32 = 291;
const SUGARS: u32 = 269;
const SAT_FAT: u32 = 606;
const SODIUM_MG: u32 = 307;
const WANTED: &[u32] = &[
    KCAL,
    KJ,
    PROTEIN,
    FAT,
    CARBS_BY_DIFFERENCE,
    FIBER,
    SUGARS,
    SAT_FAT,
    SODIUM_MG,
];

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let mut zip = Zip::open(&files[0])?;
    let names = decode_text(&zip.read_named("FOOD NAME.csv")?);
    let amounts = decode_text(&zip.read_named("NUTRIENT AMOUNT.csv")?);
    parse_tables(&names, &amounts)
}

fn parse_tables(names_csv: &str, amounts_csv: &str) -> Result<Vec<DatasetFood>, ImportError> {
    let mut values: HashMap<String, HashMap<u32, f64>> = HashMap::new();
    let mut table = Table::new(amounts_csv, ',', "NUTRIENT AMOUNT.csv")?;
    let (id_col, nutrient_col, value_col) = (
        table.column("FoodID")?,
        table.column("NutrientID")?,
        table.column("NutrientValue")?,
    );
    for row in table.rows.by_ref() {
        let Ok(nutrient) = field(&row, nutrient_col).parse::<u32>() else {
            continue;
        };
        if WANTED.contains(&nutrient)
            && let Value::Number(v) = Value::parse(field(&row, value_col))
        {
            values
                .entry(field(&row, id_col).to_string())
                .or_default()
                .entry(nutrient)
                .or_insert(v);
        }
    }

    let mut table = Table::new(names_csv, ',', "FOOD NAME.csv")?;
    let (id_col, en_col, fr_col) = (
        table.column("FoodID")?,
        table.column("FoodDescription")?,
        table.column("FoodDescriptionF")?,
    );
    let empty = HashMap::new();
    let mut foods = Vec::new();
    for row in table.rows.by_ref() {
        let id = field(&row, id_col);
        if id.is_empty() {
            continue;
        }
        let v = values.get(id).unwrap_or(&empty);
        let get = |n: u32| Value::from(v.get(&n).copied());
        let mut b = FoodBuilder::new(id, Basis::Per100g);
        b.name(Lang::En, field(&row, en_col));
        b.name(Lang::Fr, field(&row, fr_col));
        b.set(Nutrient::Energy, get(KCAL));
        b.energy_kj(get(KJ));
        b.set(Nutrient::Protein, get(PROTEIN));
        b.set(Nutrient::Fat, get(FAT));
        if b.set(Nutrient::Carbs, get(CARBS_BY_DIFFERENCE)) {
            b.carbs_include_fiber();
        }
        b.set(Nutrient::Fiber, get(FIBER));
        b.set(Nutrient::Sugars, get(SUGARS));
        b.set(Nutrient::SatFat, get(SAT_FAT));
        b.sodium_mg(get(SODIUM_MG));
        foods.extend(b.build());
    }
    Ok(foods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::archive::tests::zip_of;

    // Rows from the 2015 release.
    const NAMES: &str = "FoodID,FoodCode,FoodGroupID,FoodSourceID,FoodDescription,FoodDescriptionF,FoodDateOfEntry,FoodDateOfPublication,CountryCode,ScientificName\r\n\
2,2,22,20,Cheese souffle,Soufflé au fromage,1981-01-01,,,\r\n\
4,4,22,20,\"Chop suey, with meat, canned\",\"Chop suey, avec viande, conserve\",1981-01-01,,,\r\n";
    const AMOUNTS: &str = "FoodID,NutrientID,NutrientValue,StandardError,NumberofObservations,NutrientSourceID,NutrientDateOfEntry\r\n\
2,203,9.54,0,0,102,2010-04-16\r\n\
2,204,15.7,0,0,102,2010-04-16\r\n\
2,205,5.91,0,0,102,2010-04-16\r\n\
2,208,204,0,0,102,2010-04-16\r\n\
2,268,853,0,0,4,2010-04-16\r\n\
2,269,2.66,,,102,2010-04-16\r\n\
2,291,0.1,0,0,102,2010-04-16\r\n\
2,307,318,0,0,102,2010-04-16\r\n\
2,606,5.742,0,0,102,2010-04-16\r\n\
4,268,251,0,0,4,2010-04-19\r\n";

    #[test]
    fn reads_bilingual_names_and_values() {
        let foods = parse_tables(NAMES, AMOUNTS).unwrap();
        let souffle = &foods[0];
        assert_eq!(souffle.code, "2");
        assert_eq!(
            souffle.names,
            [
                (Lang::En, "Cheese souffle".to_string()),
                (Lang::Fr, "Soufflé au fromage".to_string()),
            ]
        );
        assert_eq!(souffle.nutrients.energy_kcal, Some(204.0));
        assert_eq!(souffle.nutrients.carbs_g, Some(5.91));
        assert_eq!(souffle.nutrients.fiber_g, Some(0.1));
        assert!(souffle.carbs_include_fiber);
        assert!((souffle.nutrients.salt_g.unwrap() - 0.795).abs() < 1e-9);
        assert!(!souffle.energy_from_kj, "kcal is preferred");

        // Only the kJ row is kept here, to check the conversion.
        let chop_suey = &foods[1];
        assert_eq!(chop_suey.names[0].1, "Chop suey, with meat, canned");
        assert!(chop_suey.energy_from_kj);
        assert_eq!(chop_suey.nutrients.protein_g, None);
    }

    #[test]
    fn reads_latin1_zip() {
        let names = encoding_rs::WINDOWS_1252.encode(NAMES).0.into_owned();
        let z = zip_of(&[
            ("FOOD NAME.csv", &names),
            ("NUTRIENT AMOUNT.csv", AMOUNTS.as_bytes()),
        ]);
        let foods = parse(&[z]).unwrap();
        assert_eq!(foods[0].names[1].1, "Soufflé au fromage");
    }
}
