//! McCance and Widdowson's Composition of Foods Integrated Dataset (UK),
//! from the Excel download.
//!
//! Each sheet has a header row, a row of nutrient codes (PROT, FAT, …), a
//! row of descriptions, then one row per food keyed by "Food Code". Values
//! are per 100 g, except alcoholic drinks (food groups starting with "Q"),
//! which are per 100 ml. "Tr" marks traces and "N" a nutrient that is
//! present but not determined.

use std::collections::HashMap;

use calamine::Data;

use super::archive::{UploadFile, cell_text, cell_value, sheet_rows, workbook};
use super::value::{FoodBuilder, Value};
use super::{DatasetFood, ImportError};
use crate::i18n::Lang;
use crate::nutrition::{Basis, Nutrient};

const PROXIMATES: &str = "1.3 Proximates";
const INORGANICS: &str = "1.4 Inorganics";

pub fn parse(files: &[UploadFile]) -> Result<Vec<DatasetFood>, ImportError> {
    let mut wb = workbook(&files[0])?;
    let proximates = sheet_rows(&mut wb, PROXIMATES, |n| n.contains("proximates"))?;
    let inorganics = sheet_rows(&mut wb, INORGANICS, |n| n.contains("inorganics"))?;
    parse_sheets(&proximates, &inorganics)
}

/// One sheet: nutrient columns by code, and data rows by food code.
struct Sheet<'a> {
    codes: HashMap<String, usize>,
    code_col: usize,
    name_col: usize,
    group_col: Option<usize>,
    data: &'a [Vec<Data>],
}

impl<'a> Sheet<'a> {
    fn new(rows: &'a [Vec<Data>], file: &'static str) -> Result<Self, ImportError> {
        let format = |detail: &str| ImportError::Format {
            file: file.into(),
            detail: detail.into(),
        };
        let header_row = rows
            .iter()
            .position(|r| r.iter().any(|c| cell_text(c) == "Food Name"))
            .ok_or_else(|| format("no Food Name header"))?;
        let header = &rows[header_row];
        let name_col = header
            .iter()
            .position(|c| cell_text(c) == "Food Name")
            .ok_or_else(|| format("no Food Name header"))?;
        // The food code column is first, but its header is blank in some sheets.
        let code_col = name_col
            .checked_sub(1)
            .ok_or_else(|| format("no Food Code column"))?;
        let group_col = header.iter().position(|c| cell_text(c) == "Group");
        let codes = rows
            .get(header_row + 1)
            .ok_or_else(|| format("no nutrient code row"))?
            .iter()
            .enumerate()
            .map(|(i, c)| (cell_text(c), i))
            .filter(|(c, _)| !c.is_empty())
            .collect();
        Ok(Sheet {
            codes,
            code_col,
            name_col,
            group_col,
            data: &rows[header_row + 2..],
        })
    }

    fn column(&self, code: &str, file: &'static str) -> Result<usize, ImportError> {
        self.codes
            .get(code)
            .copied()
            .ok_or_else(|| ImportError::Format {
                file: file.into(),
                detail: format!("no {code} column"),
            })
    }

    /// Data rows with a food code.
    fn foods(&self) -> impl Iterator<Item = (String, &'a [Data])> + '_ {
        self.data.iter().filter_map(|r| {
            let code = cell_text(r.get(self.code_col)?);
            (!code.is_empty()).then_some((code, r.as_slice()))
        })
    }
}

fn get(row: &[Data], col: usize) -> Value {
    row.get(col).map(cell_value).unwrap_or(Value::Missing)
}

fn parse_sheets(
    proximates: &[Vec<Data>],
    inorganics: &[Vec<Data>],
) -> Result<Vec<DatasetFood>, ImportError> {
    let p = Sheet::new(proximates, PROXIMATES)?;
    let col = |code| p.column(code, PROXIMATES);
    let (protein, fat, carbs, kcal, kj, fiber, sugars, sat_fat) = (
        col("PROT")?,
        col("FAT")?,
        col("CHO")?,
        col("KCALS")?,
        col("KJ")?,
        col("AOACFIB")?,
        col("TOTSUG")?,
        col("SATFOD")?,
    );

    let i = Sheet::new(inorganics, INORGANICS)?;
    let sodium_col = i.column("NA", INORGANICS)?;
    let sodium: HashMap<String, Value> = i
        .foods()
        .map(|(code, row)| (code, get(row, sodium_col)))
        .collect();

    let mut foods = Vec::new();
    for (code, row) in p.foods() {
        let group = p
            .group_col
            .and_then(|g| row.get(g))
            .map(cell_text)
            .unwrap_or_default();
        let basis = if group.starts_with('Q') {
            Basis::Per100ml
        } else {
            Basis::Per100g
        };
        let mut b = FoodBuilder::new(code.clone(), basis);
        b.name(
            Lang::En,
            &row.get(p.name_col).map(cell_text).unwrap_or_default(),
        );
        b.set(Nutrient::Energy, get(row, kcal));
        b.energy_kj(get(row, kj));
        b.set(Nutrient::Protein, get(row, protein));
        b.set(Nutrient::Fat, get(row, fat));
        b.set(Nutrient::Carbs, get(row, carbs));
        b.set(Nutrient::Fiber, get(row, fiber));
        b.set(Nutrient::Sugars, get(row, sugars));
        b.set(Nutrient::SatFat, get(row, sat_fat));
        b.sodium_mg(sodium.get(&code).copied().unwrap_or(Value::Missing));
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

    fn f(v: f64) -> Data {
        Data::Float(v)
    }

    /// The layout and two food rows of the 2021 release: header, codes,
    /// descriptions, foods (other columns left out).
    fn proximates() -> Vec<Vec<Data>> {
        let head = ["Food Code", "Food Name", "Description", "Group", "Previous"];
        let codes = [
            "PROT", "FAT", "CHO", "KCALS", "KJ", "AOACFIB", "TOTSUG", "SATFOD",
        ];
        let mut header: Vec<Data> = head.iter().map(|h| s(h)).collect();
        header.extend(codes.iter().map(|_| s("x (g)")));
        let mut code_row = vec![Data::Empty; head.len()];
        code_row.extend(codes.iter().map(|c| s(c)));
        let mut desc_row = vec![Data::Empty; head.len()];
        desc_row.extend(codes.iter().map(|_| s("description")));
        let ackee = vec![
            s("13-145"),
            s("Ackee, canned, drained"),
            s("8 cans"),
            s("DG"),
            s("554"),
            f(2.9),
            f(15.2),
            f(0.8),
            f(151.0),
            f(625.0),
            Data::Empty,
            f(0.8),
            s("N"),
        ];
        let beer = vec![
            s("17-506"),
            s("Beer, bitter, average (<4% ABV)"),
            Data::Empty,
            s("QA"),
            Data::Empty,
            f(0.3),
            s("Tr"),
            f(2.2),
            f(30.0),
            f(124.0),
            s("Tr"),
            f(2.2),
            s("Tr"),
        ];
        vec![header, code_row, desc_row, ackee, beer]
    }

    fn inorganics() -> Vec<Vec<Data>> {
        vec![
            vec![s("  "), s("Food Name"), s("Description"), s("Sodium (mg)")],
            vec![Data::Empty, Data::Empty, Data::Empty, s("NA")],
            vec![Data::Empty, Data::Empty, Data::Empty, s("Sodium")],
            vec![
                s("13-145"),
                s("Ackee, canned, drained"),
                s("8 cans"),
                f(240.0),
            ],
            vec![
                s("17-506"),
                s("Beer, bitter, average (<4% ABV)"),
                Data::Empty,
                f(6.0),
            ],
        ]
    }

    #[test]
    fn reads_codes_markers_and_basis() {
        let foods = parse_sheets(&proximates(), &inorganics()).unwrap();
        assert_eq!(foods.len(), 2, "the description row is not a food");

        let ackee = &foods[0];
        assert_eq!(ackee.code, "13-145");
        assert_eq!(ackee.basis, Basis::Per100g);
        assert_eq!(ackee.nutrients.energy_kcal, Some(151.0));
        assert_eq!(ackee.nutrients.fat_g, Some(15.2));
        assert_eq!(ackee.nutrients.fiber_g, None);
        assert_eq!(ackee.nutrients.sat_fat_g, None, "N means not determined");
        assert!((ackee.nutrients.salt_g.unwrap() - 0.6).abs() < 1e-9);

        let beer = &foods[1];
        assert_eq!(
            beer.basis,
            Basis::Per100ml,
            "alcoholic drinks are per 100 ml"
        );
        assert_eq!(beer.nutrients.protein_g, Some(0.3));
        assert_eq!(beer.nutrients.fat_g, Some(0.0));
        assert_eq!(
            beer.trace,
            [Nutrient::Fat, Nutrient::Fiber, Nutrient::SatFat]
        );
        assert!((beer.nutrients.salt_g.unwrap() - 0.015).abs() < 1e-9);
    }

    #[test]
    fn missing_nutrient_column_is_a_format_error() {
        let mut p = proximates();
        p[1][5] = s("PROTEIN");
        assert!(matches!(
            parse_sheets(&p, &inorganics()),
            Err(ImportError::Format { .. })
        ));
    }
}
