//! Uploaded files: zip archives as published, spreadsheets and text in
//! legacy encodings.

use std::io::{Cursor, Read};

use super::ImportError;

/// Largest uncompressed size read from one archive member, so a malformed
/// or hostile archive cannot exhaust memory. The largest member any
/// supported table needs (USDA SR Legacy `food_nutrient.csv`) is ~36 MB.
const MAX_MEMBER_BYTES: u64 = 256 * 1024 * 1024;

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
    calamine::open_workbook_from_rs(Cursor::new(file.bytes.as_slice()))
        .map_err(|e: calamine::XlsxError| ImportError::BadArchive(format!("{}: {e}", file.name)))
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
    let range = wb.worksheet_range(&name).map_err(|e| ImportError::Format {
        file: name.clone(),
        detail: e.to_string(),
    })?;
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
    fn decodes_legacy_text() {
        assert_eq!(decode_text("Soufflé".as_bytes()), "Soufflé");
        assert_eq!(decode_text(b"Souffl\xe9 \x80"), "Soufflé €");
    }
}
