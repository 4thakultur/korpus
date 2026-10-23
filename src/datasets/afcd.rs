//! Australian Food Composition Database (FSANZ), from the Excel file
//! "Nutrient profiles".
//!
//! One sheet gives every food per 100 g; a second gives liquids per 100 ml.
//! Liquids are taken per 100 ml so drinks are logged by volume. Columns are
//! found by their header text; energy is published in kJ only.

use std::collections::HashMap;

use calamine::Data;

use super::archive::{UploadFile, cell_text, cell_value, sheet_rows, workbook};
use super::value::{FoodBuilder, Value};
use super::{DatasetFood, ImportError};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient};

const SOLIDS: &str = "All solids & liquids per 100 g";
const LIQUIDS: &str = "Liquids only per 100 mL";

const KEY: &str = "public food key";
const NAME: &str = "food name";
const KJ: &str = "energy with dietary fibre, equated (kj)";
const PROTEIN: &str = "protein (g)";
const FAT: &str = "fat, total (g)";
const CARBS: &str = "available carbohydrate, with sugar alcohols (g)";
const FIBER: &str = "total dietary fibre (g)";
const SUGARS: &str = "total sugars (g)";
const SAT_FAT: &str = "total saturated fatty acids, equated (g)";
const SODIUM_MG: &str = "sodium (na) (mg)";

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let mut wb = workbook(&files[0])?;
    let solids = sheet_rows(&mut wb, SOLIDS, |n| n.contains("per 100 g"))?;
    let liquids = sheet_rows(&mut wb, LIQUIDS, |n| n.contains("per 100 ml"))?;
    parse_sheets(&solids, &liquids)
}

fn parse_sheets(
    solids: &[Vec<Data>],
    liquids: &[Vec<Data>],
) -> Result<Vec<DatasetFood>, ImportError> {
    let liquids: HashMap<String, DatasetFood> = read_sheet(liquids, Basis::Per100ml, LIQUIDS)?
        .into_iter()
        .map(|f| (f.code.clone(), f))
        .collect();
    let mut foods = read_sheet(solids, Basis::Per100g, SOLIDS)?;
    for f in &mut foods {
        if let Some(liquid) = liquids.get(&f.code) {
            *f = liquid.clone();
        }
    }
    Ok(foods)
}

fn read_sheet(
    rows: &[Vec<Data>],
    basis: Basis,
    file: &'static str,
) -> Result<Vec<DatasetFood>, ImportError> {
    let header_row = rows
        .iter()
        .position(|r| {
            r.first()
                .is_some_and(|c| cell_text(c).eq_ignore_ascii_case(KEY))
        })
        .ok_or_else(|| ImportError::Format {
            file: file.into(),
            detail: "no Public Food Key header".into(),
        })?;
    let header: Vec<String> = rows[header_row]
        .iter()
        .map(|c| cell_text(c).to_lowercase())
        .collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| ImportError::Format {
                file: file.into(),
                detail: format!("no column {name}"),
            })
    };
    let (key, name, kj, protein, fat, carbs, fiber, sugars, sat_fat, sodium) = (
        col(KEY)?,
        col(NAME)?,
        col(KJ)?,
        col(PROTEIN)?,
        col(FAT)?,
        col(CARBS)?,
        col(FIBER)?,
        col(SUGARS)?,
        col(SAT_FAT)?,
        col(SODIUM_MG)?,
    );

    let mut foods = Vec::new();
    for row in &rows[header_row + 1..] {
        let get = |i: usize| row.get(i).map(cell_value).unwrap_or(Value::Missing);
        let code = row.get(key).map(cell_text).unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        let mut b = FoodBuilder::new(code, basis);
        b.name(Lang::En, &row.get(name).map(cell_text).unwrap_or_default());
        b.energy_kj(get(kj));
        b.set(Nutrient::Protein, get(protein));
        b.set(Nutrient::Fat, get(fat));
        b.set(Nutrient::Carbs, get(carbs));
        b.set(Nutrient::Fiber, get(fiber));
        b.set(Nutrient::Sugars, get(sugars));
        b.set(Nutrient::SatFat, get(sat_fat));
        b.sodium_mg(get(sodium));
        foods.extend(b.build());
    }
    Ok(foods)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Data {
        Data::String(v.into())
    }

    /// Release 3 layout: a title row, a blank row, headers with line
    /// breaks and doubled spaces, then foods.
    fn sheet(title: &str, foods: Vec<Vec<Data>>) -> Vec<Vec<Data>> {
        let header = [
            "Public Food Key",
            "Classification",
            "Derivation",
            "Food Name",
            "Energy with dietary fibre, equated \n(kJ)",
            "Protein \n(g)",
            "Fat, total \n(g)",
            "Available carbohydrate, with sugar alcohols \n(g)",
            "Total dietary fibre \n(g)",
            "Total sugars (g)",
            "Total saturated fatty acids, equated  (g)",
            "Sodium (Na) \n(mg)",
        ];
        let mut rows = vec![
            vec![s(title)],
            vec![Data::Empty],
            header.iter().map(|h| s(h)).collect(),
        ];
        rows.extend(foods);
        rows
    }

    /// A food row; the values after protein are cardamom's.
    fn food(key: &str, name: &str, kj: f64, protein: Data) -> Vec<Data> {
        vec![
            s(key),
            Data::Float(31302.0),
            s("Borrowed"),
            s(name),
            Data::Float(kj),
            protein,
            Data::Float(6.7),
            Data::Float(34.4),
            Data::Float(28.0),
            Data::Float(4.4),
            Data::Float(2.2),
            Data::Float(18.0),
        ]
    }

    #[test]
    fn reads_both_sheets_and_prefers_volume_for_liquids() {
        let solids = sheet(
            "Release 3 - Nutrient profiles (per 100 g)",
            vec![
                food(
                    "F002258",
                    "Cardamom seed, dried, ground",
                    1236.0,
                    Data::Float(10.8),
                ),
                food(
                    "F009351",
                    "Vanilla, artificial or imitation",
                    231.0,
                    Data::Float(0.0),
                ),
                vec![Data::Empty],
            ],
        );
        let liquids = sheet(
            "Release 3 - Nutrient profiles (per 100 mL)",
            vec![food(
                "F009351",
                "Vanilla, artificial or imitation",
                231.0,
                Data::Float(0.0),
            )],
        );
        let foods = parse_sheets(&solids, &liquids).unwrap();
        assert_eq!(foods.len(), 2);

        let cardamom = &foods[0];
        assert_eq!(cardamom.basis, Basis::Per100g);
        assert!(cardamom.energy_from_kj);
        assert!((cardamom.nutrients.energy_kcal.unwrap() - 1236.0 / 4.184).abs() < 1e-9);
        assert_eq!(cardamom.nutrients.carbs_g, Some(34.4));
        assert_eq!(cardamom.nutrients.sat_fat_g, Some(2.2));
        assert!((cardamom.nutrients.salt_g.unwrap() - 0.045).abs() < 1e-9);

        let vanilla = &foods[1];
        assert_eq!(vanilla.basis, Basis::Per100ml);
        assert_eq!(vanilla.nutrients.protein_g, Some(0.0));
    }

    #[test]
    fn renamed_column_is_a_format_error() {
        let mut solids = sheet("t", vec![]);
        solids[2][6] = s("Fat (g)");
        assert!(matches!(
            parse_sheets(&solids, &sheet("t", vec![])),
            Err(ImportError::Format { .. })
        ));
    }
}
