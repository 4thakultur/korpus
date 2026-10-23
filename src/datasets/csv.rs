//! A small RFC 4180 CSV reader: quoted fields, doubled quotes, line breaks
//! inside quotes, CRLF or LF. Enough for the published tables without
//! another dependency.

use super::ImportError;

pub struct Csv<'a> {
    rest: &'a str,
    delimiter: char,
}

impl<'a> Csv<'a> {
    pub fn new(text: &'a str, delimiter: char) -> Self {
        Csv {
            rest: text.strip_prefix('\u{feff}').unwrap_or(text),
            delimiter,
        }
    }

    fn record(&mut self) -> Option<Vec<String>> {
        if self.rest.is_empty() {
            return None;
        }
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = self.rest.char_indices().peekable();
        let mut end = self.rest.len();
        while let Some((i, c)) = chars.next() {
            if quoted {
                if c == '"' {
                    if chars.peek().is_some_and(|(_, n)| *n == '"') {
                        field.push('"');
                        chars.next();
                    } else {
                        quoted = false;
                    }
                } else {
                    field.push(c);
                }
            } else if c == '"' {
                quoted = true;
            } else if c == self.delimiter {
                fields.push(std::mem::take(&mut field));
            } else if c == '\n' {
                end = i + 1;
                break;
            } else if c != '\r' {
                field.push(c);
            }
        }
        fields.push(field);
        self.rest = &self.rest[end..];
        Some(fields)
    }
}

impl Iterator for Csv<'_> {
    type Item = Vec<String>;

    fn next(&mut self) -> Option<Vec<String>> {
        loop {
            let r = self.record()?;
            // Skip blank lines.
            if !(r.len() == 1 && r[0].trim().is_empty()) {
                return Some(r);
            }
        }
    }
}

/// A CSV file with a header row; columns are looked up by name.
pub struct Table<'a> {
    pub rows: Csv<'a>,
    header: Vec<String>,
    file: &'static str,
}

impl<'a> Table<'a> {
    pub fn new(text: &'a str, delimiter: char, file: &'static str) -> Result<Self, ImportError> {
        let mut rows = Csv::new(text, delimiter);
        let header = rows.next().ok_or_else(|| ImportError::Format {
            file: file.into(),
            detail: "empty file".into(),
        })?;
        Ok(Table {
            rows,
            header: header.into_iter().map(|h| h.trim().to_string()).collect(),
            file,
        })
    }

    /// Index of a required column (case-insensitive).
    pub fn column(&self, name: &str) -> Result<usize, ImportError> {
        self.header
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .ok_or_else(|| ImportError::Format {
                file: self.file.into(),
                detail: format!("no column {name}"),
            })
    }
}

/// The field at `i`, or "" for short rows.
pub fn field(row: &[String], i: usize) -> &str {
    row.get(i).map(|s| s.trim()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quotes_delimiters_and_line_endings() {
        let text = "\u{feff}a,b,c\r\n1,\"x, y\",\"say \"\"hi\"\"\"\r\n\n2,\"multi\nline\",\n";
        let rows: Vec<Vec<String>> = Csv::new(text, ',').collect();
        assert_eq!(rows[0], ["a", "b", "c"]);
        assert_eq!(rows[1], ["1", "x, y", "say \"hi\""]);
        assert_eq!(rows[2], ["2", "multi\nline", ""]);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn semicolons_and_missing_final_newline() {
        let rows: Vec<Vec<String>> = Csv::new("a;b\n1;2", ';').collect();
        assert_eq!(rows, [vec!["a", "b"], vec!["1", "2"]]);
    }

    #[test]
    fn finds_columns_by_name() {
        let t = Table::new("FoodID, NutrientValue\n1,2\n", ',', "x.csv").unwrap();
        assert_eq!(t.column("nutrientvalue").unwrap(), 1);
        assert!(matches!(
            t.column("missing"),
            Err(ImportError::Format { .. })
        ));
        assert_eq!(field(&["a".into()], 3), "");
    }
}
