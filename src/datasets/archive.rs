//! Uploaded files: zip archives as published, spreadsheets and text in
//! legacy encodings.

use std::io::{Cursor, Read};

use super::ImportError;

/// Largest uncompressed size read from one archive member, so a malformed
/// or hostile archive cannot exhaust memory. The largest member any
/// supported table needs (USDA SR Legacy `food_nutrient.csv`) is ~36 MB.
const MAX_MEMBER_BYTES: u64 = 256 * 1024 * 1024;
/// Largest total uncompressed size of a workbook's parts.
const MAX_WORKBOOK_BYTES: u64 = 512 * 1024 * 1024;
/// Largest sheet area (rows × columns) read. The biggest supported sheet,
/// CoFID's, has under a million cells.
const MAX_SHEET_CELLS: u64 = 10_000_000;

/// A file as uploaded or read from disk.
pub struct UploadFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct Zip<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Zip<'a> {
    pub fn open(file: &'a UploadFile) -> Result<Zip<'a>, ImportError> {
        zip::ZipArchive::new(Cursor::new(file.bytes.as_slice()))
            .map(|archive| Zip { archive })
            .map_err(|e| ImportError::BadArchive(format!("{}: {e}", file.name)))
    }

    /// Name of the first member whose file name (ignoring folders) matches.
    fn find(&self, matches: impl Fn(&str) -> bool) -> Option<String> {
        self.archive
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .filter(|n| !n.ends_with('/'))
            .find(|n| {
                let base = n.rsplit('/').next().unwrap_or(n);
                // Skip macOS resource forks such as "__MACOSX/._food.csv".
                !base.starts_with("._") && matches(&base.to_ascii_lowercase())
            })
    }

    /// Reads the member matching `matches`, reported as `label` if missing.
    pub fn read(
        &mut self,
        label: &'static str,
        matches: impl Fn(&str) -> bool,
    ) -> Result<Vec<u8>, ImportError> {
        let name = self.find(matches).ok_or(ImportError::MissingFile(label))?;
        let member = self
            .archive
            .by_name(&name)
            .map_err(|e| ImportError::BadArchive(format!("{name}: {e}")))?;
        if member.size() > MAX_MEMBER_BYTES {
            return Err(ImportError::BadArchive(format!("{name} is too large")));
        }
        let mut out = Vec::with_capacity(member.size() as usize);
        member
            .take(MAX_MEMBER_BYTES)
            .read_to_end(&mut out)
            .map_err(|e| ImportError::BadArchive(format!("{name}: {e}")))?;
        Ok(out)
    }

    /// Like [`Zip::read`], matching one file name exactly (case-insensitive).
    pub fn read_named(&mut self, name: &'static str) -> Result<Vec<u8>, ImportError> {
        let lower = name.to_ascii_lowercase();
        self.read(name, move |n| n == lower)
    }
}

/// UTF-8 text, falling back to Windows-1252 (a superset of Latin-1), which
/// older tables use.
pub fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
    }
}

/// Opens an uploaded `.xlsx` workbook.
pub fn workbook(file: &UploadFile) -> Result<calamine::Xlsx<Cursor<&[u8]>>, ImportError> {
    check_expansion(file, MAX_MEMBER_BYTES, MAX_WORKBOOK_BYTES)?;
    calamine::open_workbook_from_rs(Cursor::new(file.bytes.as_slice()))
        .map_err(|e: calamine::XlsxError| ImportError::BadArchive(format!("{}: {e}", file.name)))
}

/// Decompresses every part of a workbook once, counting bytes, so a file
/// whose parts expand far beyond any real spreadsheet (a "zip bomb") is
/// refused before the spreadsheet reader loads them into memory. Declared
/// sizes are not trusted: they can be forged.
fn check_expansion(file: &UploadFile, max_member: u64, max_total: u64) -> Result<(), ImportError> {
    let bad = |detail: String| ImportError::BadArchive(format!("{}: {detail}", file.name));
    let mut archive =
        zip::ZipArchive::new(Cursor::new(file.bytes.as_slice())).map_err(|e| bad(e.to_string()))?;
    let mut total = 0u64;
    let mut buf = vec![0u8; 64 * 1024];
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).map_err(|e| bad(e.to_string()))?;
        let mut size = 0u64;
        loop {
            let n = member.read(&mut buf).map_err(|e| bad(e.to_string()))? as u64;
            if n == 0 {
                break;
            }
            size += n;
            total += n;
            if size > max_member || total > max_total {
                return Err(bad("uncompressed contents are too large".into()));
            }
        }
    }
    Ok(())
}

/// The rows of the first sheet whose name satisfies `matches`.
pub fn sheet_rows(
    wb: &mut calamine::Xlsx<Cursor<&[u8]>>,
    label: &'static str,
    matches: impl Fn(&str) -> bool,
) -> Result<Vec<Vec<calamine::Data>>, ImportError> {
    use calamine::Reader;
    let name = wb
        .sheet_names()
        .into_iter()
        .find(|n| matches(&n.to_lowercase()))
        .ok_or(ImportError::MissingFile(label))?;
    let format = |detail: String| ImportError::Format {
        file: name.clone(),
        detail,
    };
    // Read cell by cell rather than with `worksheet_range`, which allocates
    // the whole area between the first and last cell: two cells in opposite
    // corners would ask for billions.
    let mut reader = wb
        .worksheet_cells_reader(&name)
        .map_err(|e| format(e.to_string()))?;
    let mut cells = Vec::new();
    let (mut rows, mut cols) = ((u32::MAX, 0), (u32::MAX, 0));
    while let Some(cell) = reader.next_cell().map_err(|e| format(e.to_string()))? {
        let value = calamine::Data::from(cell.get_value().clone());
        if value == calamine::Data::Empty {
            continue;
        }
        let (r, c) = cell.get_position();
        rows = (rows.0.min(r), rows.1.max(r));
        cols = (cols.0.min(c), cols.1.max(c));
        let area = u64::from(rows.1 - rows.0 + 1) * u64::from(cols.1 - cols.0 + 1);
        if area > MAX_SHEET_CELLS {
            return Err(format("sheet is too large".into()));
        }
        cells.push(calamine::Cell::new((r, c), value));
    }
    let range = calamine::Range::from_sparse(cells);
    Ok(range.rows().map(<[calamine::Data]>::to_vec).collect())
}

/// A cell as text, with whitespace (including line breaks in headers)
/// collapsed.
pub fn cell_text(cell: &calamine::Data) -> String {
    use calamine::Data;
    let s = match cell {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        _ => String::new(),
    };
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn cell_value(cell: &calamine::Data) -> super::value::Value {
    use super::value::Value;
    use calamine::Data;
    match cell {
        Data::Float(f) => Value::from(Some(*f)),
        Data::Int(i) => Value::Number(*i as f64),
        Data::String(s) => Value::parse(s),
        _ => Value::Missing,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip archive with the given members, for parser tests.
    pub fn zip_of(members: &[(&str, &[u8])]) -> UploadFile {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in members {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(bytes).unwrap();
        }
        UploadFile {
            name: "test.zip".into(),
            bytes: w.finish().unwrap().into_inner(),
        }
    }

    #[test]
    fn finds_members_in_folders_case_insensitively() {
        let f = zip_of(&[
            ("__MACOSX/._FOOD NAME.csv", b"junk"),
            ("cnf/FOOD NAME.csv", b"ok"),
        ]);
        let mut z = Zip::open(&f).unwrap();
        assert_eq!(z.read_named("food name.csv").unwrap(), b"ok");
        assert_eq!(
            z.read_named("missing.csv"),
            Err(ImportError::MissingFile("missing.csv"))
        );
    }

    #[test]
    fn rejects_non_zip_files() {
        let f = UploadFile {
            name: "x.txt".into(),
            bytes: b"hello".to_vec(),
        };
        assert!(matches!(Zip::open(&f), Err(ImportError::BadArchive(_))));
    }

    #[test]
    fn refuses_workbooks_that_expand_too_far() {
        let zeros = vec![0u8; 1024 * 1024];
        let bomb = zip_of(&[("a.xml", &zeros), ("b.xml", &zeros)]);
        assert!(bomb.bytes.len() < 16 * 1024, "compresses well");
        let mib = 1024 * 1024;
        assert!(check_expansion(&bomb, mib, 2 * mib).is_ok());
        for (member, total) in [(mib - 1, 2 * mib), (mib, 2 * mib - 1)] {
            assert!(matches!(
                check_expansion(&bomb, member, total),
                Err(ImportError::BadArchive(_))
            ));
        }
    }

    /// A one-sheet workbook ("Data") with the given `<sheetData>` rows.
    fn xlsx_of(rows: &str) -> UploadFile {
        let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
        let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
        let workbook = format!(
            r#"<workbook xmlns="{main}" xmlns:r="{rel}"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#
        );
        let rels = format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rel}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
        );
        let sheet =
            format!(r#"<worksheet xmlns="{main}"><sheetData>{rows}</sheetData></worksheet>"#);
        let root_rels = format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{rel}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        );
        let mut f = zip_of(&[
            ("_rels/.rels", root_rels.as_bytes()),
            ("xl/workbook.xml", workbook.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ]);
        f.name = "test.xlsx".into();
        f
    }

    #[test]
    fn reads_sheet_rows_from_the_first_cell() {
        let f = xlsx_of(
            r#"<row r="2"><c r="B2" t="inlineStr"><is><t>Name</t></is></c><c r="C2"><v>1.5</v></c></row><row r="3"><c r="C3"><v>2</v></c></row>"#,
        );
        let mut wb = workbook(&f).unwrap();
        let rows = sheet_rows(&mut wb, "data", |n| n == "data").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(cell_text(&rows[0][0]), "Name");
        assert_eq!(cell_text(&rows[1][1]), "2");
    }

    #[test]
    fn refuses_sheets_spanning_a_huge_area() {
        let f = xlsx_of(
            r#"<row r="1"><c r="A1"><v>1</v></c></row><row r="1048576"><c r="XFD1048576"><v>2</v></c></row>"#,
        );
        let mut wb = workbook(&f).unwrap();
        assert!(matches!(
            sheet_rows(&mut wb, "data", |n| n == "data"),
            Err(ImportError::Format { .. })
        ));
    }

    #[test]
    fn decodes_legacy_text() {
        assert_eq!(decode_text("Soufflé".as_bytes()), "Soufflé");
        assert_eq!(decode_text(b"Souffl\xe9 \x80"), "Soufflé €");
    }
}
