//! Result-set export (ADR-0035): adapter-neutral serialization of a
//! [`QueryResult`] to delimited text — CSV for a saved file, TSV for the
//! clipboard. Pure and I/O-free, so the wire format is unit-tested
//! without a grid, a clipboard, or a file dialog. The UI layer hands the
//! output to egui's clipboard (`copy_text`) or to `rfd`'s save dialog.
//!
//! Both formats use the same RFC 4180 quoting rules (a field is quoted
//! only when it must be), which is exactly what a spreadsheet expects
//! when pasting TSV or opening a CSV. Records are separated, not
//! terminated: there is no trailing newline, so pasting TSV into Excel
//! does not leave a dangling empty row.

use std::borrow::Cow;

use dbboard_core::{Column, Row, Value};

/// Serialize the whole result as RFC 4180 CSV (comma-delimited, `\r\n`
/// records). Suited to a `.csv` file a spreadsheet will open.
#[must_use]
pub fn to_csv(columns: &[Column], rows: &[Row]) -> String {
    delimited(columns, rows.iter(), ',', "\r\n")
}

/// UTF-8 byte-order mark. Excel on Windows assumes the system ANSI code
/// page (Shift-JIS on Japanese Windows) for a BOM-less CSV and renders
/// UTF-8 text as mojibake; a leading BOM makes it auto-detect UTF-8.
/// Harmless to BOM-aware parsers and to the spreadsheet's own re-save.
pub const UTF8_BOM: &str = "\u{feff}";

/// [`to_csv`] with a leading UTF-8 BOM — the form to *write to a file* a
/// user will open in Excel. The clipboard path deliberately stays
/// BOM-less: the clipboard carries Unicode natively, and a BOM would
/// show up as a stray glyph when pasting into a plain-text target.
#[must_use]
pub fn to_csv_with_bom(columns: &[Column], rows: &[Row]) -> String {
    let mut out = String::from(UTF8_BOM);
    out.push_str(&to_csv(columns, rows));
    out
}

/// Serialize the whole result as TSV (tab-delimited, `\n` records).
/// Pastes straight into Excel / Google Sheets with columns intact.
#[must_use]
pub fn to_tsv(columns: &[Column], rows: &[Row]) -> String {
    delimited(columns, rows.iter(), '\t', "\n")
}

/// First `{stem}.{ext}` name that `exists` reports free, falling back to
/// Explorer/browser-style ` (2)`, ` (3)`, … suffixes so a second export
/// does not silently overwrite the first. `exists` is injected (rather
/// than calling the filesystem here) so the numbering is unit-tested
/// without touching disk; the caller passes a real `dir.join(n).exists()`
/// probe. The counter is bounded (a directory with tens of thousands of
/// `dbboard-result` files is not a real case); if every candidate is
/// somehow taken we return the plain name and let the OS dialog offer its
/// own overwrite prompt rather than loop forever.
#[must_use]
pub fn next_available_name(stem: &str, ext: &str, exists: impl Fn(&str) -> bool) -> String {
    let plain = format!("{stem}.{ext}");
    std::iter::once(plain.clone())
        .chain((2..=u16::MAX).map(|n| format!("{stem} ({n}).{ext}")))
        .find(|name| !exists(name))
        .unwrap_or(plain)
}

/// Shared writer: a header row of column names followed by one record
/// per row, each field escaped per RFC 4180. Generic over the row
/// iterator so the caller can pass every row or just a selected subset
/// (ADR-0035 slice 2).
fn delimited<'a>(
    columns: &[Column],
    rows: impl Iterator<Item = &'a Row>,
    delim: char,
    newline: &str,
) -> String {
    let mut out = String::new();
    push_record(
        &mut out,
        columns.iter().map(|c| Cow::Borrowed(c.name.as_str())),
        delim,
    );
    for row in rows {
        out.push_str(newline);
        push_record(&mut out, row.values().iter().map(field_text), delim);
    }
    out
}

/// Join one record's fields with `delim`, escaping each as needed.
fn push_record<'a, I>(out: &mut String, fields: I, delim: char)
where
    I: Iterator<Item = Cow<'a, str>>,
{
    let mut first = true;
    for field in fields {
        if !first {
            out.push(delim);
        }
        first = false;
        out.push_str(&escape_field(&field, delim));
    }
}

/// Render one cell for a delimited file. `NULL` becomes an empty field
/// (what spreadsheets expect) rather than the literal "NULL" the
/// [`Value`] `Display` impl uses for the grid. Everything else reuses
/// that `Display`, so a `Blob` still shows its `<blob: N bytes>`
/// placeholder — round-tripping binary through CSV is out of scope.
fn field_text(value: &Value) -> Cow<'_, str> {
    if value.is_null() {
        Cow::Borrowed("")
    } else {
        Cow::Owned(value.to_string())
    }
}

/// Quote a field only when it carries the delimiter, a quote, or a line
/// break; double any embedded quote. A field that needs none of this is
/// returned untouched (no allocation).
fn escape_field(s: &str, delim: char) -> Cow<'_, str> {
    if s.contains(delim) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let mut escaped = String::with_capacity(s.len() + 2);
        escaped.push('"');
        for c in s.chars() {
            if c == '"' {
                escaped.push('"');
            }
            escaped.push(c);
        }
        escaped.push('"');
        Cow::Owned(escaped)
    } else {
        Cow::Borrowed(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{to_csv, to_tsv};
    use dbboard_core::{Column, Row, Value};

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            declared_type: None,
        }
    }

    #[test]
    fn csv_writes_header_and_rows() {
        let columns = [col("id"), col("name")];
        let rows = [
            Row::new(vec![Value::Integer(1), Value::Text("Vegas Gift".into())]),
            Row::new(vec![Value::Integer(2), Value::Text("Cabaret".into())]),
        ];
        assert_eq!(
            to_csv(&columns, &rows),
            "id,name\r\n1,Vegas Gift\r\n2,Cabaret"
        );
    }

    #[test]
    fn tsv_uses_tabs_and_newline_records() {
        let columns = [col("id"), col("name")];
        let rows = [Row::new(vec![
            Value::Integer(1),
            Value::Text("Cabaret".into()),
        ])];
        assert_eq!(to_tsv(&columns, &rows), "id\tname\n1\tCabaret");
    }

    #[test]
    fn null_becomes_an_empty_field_not_the_word_null() {
        let columns = [col("a"), col("b")];
        let rows = [Row::new(vec![Value::Null, Value::Integer(7)])];
        assert_eq!(to_csv(&columns, &rows), "a,b\r\n,7");
    }

    #[test]
    fn quotes_fields_containing_the_delimiter_quote_or_newline() {
        let columns = [col("note")];
        let rows = [
            Row::new(vec![Value::Text("a,b".into())]),
            Row::new(vec![Value::Text("say \"hi\"".into())]),
            Row::new(vec![Value::Text("line1\nline2".into())]),
        ];
        assert_eq!(
            to_csv(&columns, &rows),
            "note\r\n\"a,b\"\r\n\"say \"\"hi\"\"\"\r\n\"line1\nline2\""
        );
    }

    #[test]
    fn tsv_quotes_fields_containing_a_tab() {
        let columns = [col("note")];
        let rows = [Row::new(vec![Value::Text("a\tb".into())])];
        assert_eq!(to_tsv(&columns, &rows), "note\n\"a\tb\"");
    }

    #[test]
    fn header_only_when_there_are_no_rows() {
        let columns = [col("id"), col("name")];
        let rows: [Row; 0] = [];
        assert_eq!(to_csv(&columns, &rows), "id,name");
    }

    #[test]
    fn csv_with_bom_prefixes_the_utf8_byte_order_mark() {
        use super::{to_csv, to_csv_with_bom, UTF8_BOM};
        let columns = [col("id")];
        let rows = [Row::new(vec![Value::Integer(1)])];
        let with = to_csv_with_bom(&columns, &rows);
        // The BOM leads, then the exact same CSV body follows.
        assert!(with.starts_with(UTF8_BOM));
        assert_eq!(&with[UTF8_BOM.len()..], to_csv(&columns, &rows));
        // Concretely: EF BB BF in UTF-8.
        assert_eq!(&with.as_bytes()[..3], &[0xEF, 0xBB, 0xBF]);
    }

    #[test]
    fn next_available_name_uses_the_plain_name_when_free() {
        use super::next_available_name;
        assert_eq!(
            next_available_name("dbboard-result", "csv", |_| false),
            "dbboard-result.csv"
        );
    }

    #[test]
    fn next_available_name_appends_explorer_style_suffix_on_collision() {
        use super::next_available_name;
        // The plain name and " (2)" are taken; " (3)" is free.
        let taken = ["dbboard-result.csv", "dbboard-result (2).csv"];
        assert_eq!(
            next_available_name("dbboard-result", "csv", |n| taken.contains(&n)),
            "dbboard-result (3).csv"
        );
    }

    #[test]
    fn real_values_render_as_numbers() {
        let columns = [col("x")];
        let rows = [Row::new(vec![Value::Real(1.5)])];
        assert_eq!(to_csv(&columns, &rows), "x\r\n1.5");
    }
}
