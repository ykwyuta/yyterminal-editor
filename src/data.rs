//! Bounded pages over shared YY snapshots. Field edits never reserialize other fields.
use anyhow::{Context, Result, bail, ensure};
use std::{ops::Range, path::Path, sync::Arc};
use yy_buffer::Snapshot;
use yy_core::{Document, EditKind, Selection, SelectionSet, Version, edit::Change};
use yy_delimited::{Dialect, RecordReader, quote_field, unquote};
use yy_sheet::fixed::{FixedSpec, RecordSep};

pub const PAGE_ROWS: usize = 128;
pub const PAGE_BYTES: u64 = 1024 * 1024;
#[derive(Clone)]
pub enum Mode {
    Delimited(Dialect),
    Fixed(Arc<FixedSpec>),
}
impl Mode {
    pub fn name(&self) -> String {
        match self {
            Self::Delimited(d) => format!("Delimited ({})", d.name()),
            Self::Fixed(spec) => format!(
                "Fixed {} bytes / {}",
                spec.data_len,
                spec.codec.charset.name()
            ),
        }
    }
    pub fn headers(&self, columns: usize) -> Vec<String> {
        match self {
            Self::Delimited(_) => (0..columns)
                .map(|i| yy_core::csv::column_name(i as u32))
                .collect(),
            Self::Fixed(spec) => spec
                .layout
                .fields
                .iter()
                .map(|f| format!("{} [{}:{}]", f.name, f.offset + 1, f.len))
                .collect(),
        }
    }
}
#[derive(Clone)]
pub struct Row {
    pub range: Range<u64>,
    pub fields: Vec<Vec<u8>>,
}
pub struct Page {
    pub version: Version,
    pub rows: Vec<Row>,
    pub next: u64,
    pub eof: bool,
    pub notice: String,
}
pub struct Grid {
    pub error: Option<String>,
    pub mode: Mode,
    pub page: Option<Page>,
    pub start: u64,
    pub base: u64,
    pub row: usize,
    pub anchor: Option<(usize, usize)>,
    pub width: u16,
    pub height: u16,
    pub widths: std::collections::BTreeMap<usize, u16>,
    pub heights: std::collections::BTreeMap<u64, u16>,
    pub col: usize,
    pub left: usize,
    pub top: usize,
    history: Vec<(u64, u64)>,
}
impl Grid {
    pub fn new(mode: Mode) -> Self {
        Self {
            error: None,
            mode,
            page: None,
            start: 0,
            base: 0,
            row: 0,
            anchor: None,
            width: 19,
            height: 1,
            widths: Default::default(),
            heights: Default::default(),
            col: 0,
            left: 0,
            top: 0,
            history: vec![],
        }
    }
    pub fn column_width(&self, col: usize) -> u16 {
        self.widths.get(&col).copied().unwrap_or(self.width)
    }
    pub fn row_height(&self, row: usize) -> u16 {
        self.heights
            .get(&(self.base + row as u64))
            .copied()
            .unwrap_or(self.height)
    }
    pub fn rectangle(&self) -> (usize, usize, usize, usize) {
        let (r, c) = self.anchor.unwrap_or((self.row, self.col));
        (
            r.min(self.row),
            c.min(self.col),
            r.max(self.row),
            c.max(self.col),
        )
    }
    pub fn copy(&self) -> Result<String> {
        let page = self.page.as_ref().context("Page is loading")?;
        let (t, l, b, r) = self.rectangle();
        let mut out = vec![];
        ensure!(
            (b - t + 1).saturating_mul(r - l + 1) <= 100_000,
            "Copy exceeds 100,000 cells"
        );
        for row in t..=b {
            let record = page
                .rows
                .get(row)
                .context("Selection outside current page")?;
            let values = (l..=r)
                .map(|col| self.value_at(record, col))
                .collect::<Vec<_>>();
            yy_delimited::write_record(
                &values,
                &Dialect::tsv(),
                if row < b { b"\n" } else { b"" },
                &mut out,
            );
            ensure!(out.len() <= 1024 * 1024, "Copy exceeds 1 MiB");
        }
        Ok(String::from_utf8(out)?)
    }
    pub fn clear(&mut self, doc: &mut Document) -> Result<()> {
        let page = self.page.as_ref().context("Page is loading")?;
        ensure!(page.version == doc.version(), "Stale page");
        let (t, l, b, r) = self.rectangle();
        let mut changes = vec![];
        for row in t..=b {
            let record = page
                .rows
                .get(row)
                .context("Selection outside current page")?;
            for col in l..=r {
                if col < record.fields.len() {
                    changes.push(self.change(record, col, "")?);
                }
            }
        }
        let at = changes.first().map_or(0, |change| change.range.start);
        doc.apply_changes(changes, EditKind::Other, move |_| {
            SelectionSet::single(Selection::caret(at))
        });
        self.page = None;
        self.anchor = None;
        Ok(())
    }
    pub fn selected(&self) -> Option<&Row> {
        self.page.as_ref()?.rows.get(self.row)
    }
    pub fn value(&self) -> Result<String> {
        let row = self.selected().context("No record selected")?;
        let raw = row.fields.get(self.col).context("No field selected")?;
        Ok(match &self.mode {
            Mode::Delimited(d) => String::from_utf8(unquote(raw, d))?,
            Mode::Fixed(spec) => {
                let field = &spec.layout.fields[self.col];
                match spec.codec.decode(field, raw) {
                    yy_cobol::Decoded::Empty => String::new(),
                    yy_cobol::Decoded::Text(text) => text,
                    yy_cobol::Decoded::Num(n) => n.to_string(),
                    yy_cobol::Decoded::Float(n) => n.to_string(),
                    yy_cobol::Decoded::Invalid => yy_cobol::hex_text(raw),
                }
            }
        })
    }
    pub fn value_at(&self, row: &Row, col: usize) -> String {
        let Some(raw) = row.fields.get(col) else {
            return String::new();
        };
        match &self.mode {
            Mode::Delimited(d) => String::from_utf8_lossy(&unquote(raw, d)).into_owned(),
            Mode::Fixed(spec) => match spec.codec.decode(&spec.layout.fields[col], raw) {
                yy_cobol::Decoded::Empty => String::new(),
                yy_cobol::Decoded::Text(s) => s,
                yy_cobol::Decoded::Num(n) => n.to_string(),
                yy_cobol::Decoded::Float(n) => n.to_string(),
                yy_cobol::Decoded::Invalid => yy_cobol::hex_text(raw),
            },
        }
    }
    pub fn next_page(&mut self) -> bool {
        let Some(page) = &self.page else { return false };
        if page.eof || page.next <= self.start {
            return false;
        }
        self.anchor = None;
        self.history.push((self.start, self.base));
        self.start = page.next;
        self.base += page.rows.len() as u64;
        self.row = 0;
        self.top = 0;
        self.page = None;
        self.error = None;
        true
    }
    pub fn previous_page(&mut self) -> bool {
        let Some((start, base)) = self.history.pop() else {
            return false;
        };
        self.anchor = None;
        self.start = start;
        self.base = base;
        self.row = 0;
        self.top = 0;
        self.page = None;
        self.error = None;
        true
    }
    pub fn reset(&mut self) {
        self.start = 0;
        self.base = 0;
        self.row = 0;
        self.top = 0;
        self.anchor = None;
        self.history.clear();
        self.page = None;
        self.error = None;
    }
    fn change(&self, row: &Row, col: usize, value: &str) -> Result<Change> {
        ensure!(
            row.fields.get(col).is_some(),
            "Paste exceeds the record's field count"
        );
        let (range, bytes) = match &self.mode {
            Mode::Delimited(d) => {
                let start = row.range.start
                    + row.fields[..col]
                        .iter()
                        .map(|v| v.len() as u64 + d.delimiter().len() as u64)
                        .sum::<u64>();
                let bytes = if value.is_empty() && row.fields.len() == 1 {
                    vec![b'"', b'"']
                } else {
                    quote_field(value.as_bytes(), d)
                };
                (start..start + row.fields[col].len() as u64, bytes)
            }
            Mode::Fixed(spec) => {
                let field = &spec.layout.fields[col];
                if let Some((_, scale)) = field.kind.digits_scale()
                    && !value.trim().is_empty()
                    && yy_cobol::parse_hex(value, field.len).is_none()
                {
                    let number = yy_cobol::Decimal::parse(value).context("Not a decimal value")?;
                    let stored = number.truncate(scale).context("Decimal overflow")?;
                    ensure!(
                        stored.truncate(number.scale) == Some(number),
                        "Value would lose fractional digits"
                    );
                }
                let mut bytes = vec![0; field.len];
                let mut issues = yy_cobol::Issues::default();
                spec.codec
                    .encode(field, yy_cobol::Input::Text(value), &mut bytes, &mut issues);
                ensure!(
                    issues.total() == 0,
                    "Value rejected: overflow={}, truncated={}, unencodable={}, invalid={}, negative={}",
                    issues.overflow,
                    issues.truncated,
                    issues.unencodable,
                    issues.not_number,
                    issues.negative
                );
                let start = row.range.start + field.offset as u64;
                (start..start + field.len as u64, bytes)
            }
        };
        Ok(Change::replace_bytes(range, bytes))
    }
    pub fn replace(&mut self, doc: &mut Document, value: &str) -> Result<()> {
        let page = self.page.as_ref().context("Page is loading")?;
        ensure!(
            page.version == doc.version(),
            "Page changed; wait for reload"
        );
        let row = self.selected().context("No record selected")?;
        let change = self.change(row, self.col, value)?;
        let at = change.range.start;
        doc.apply_changes(vec![change], EditKind::Other, move |_| {
            SelectionSet::single(Selection::caret(at))
        });
        self.page = None;
        Ok(())
    }
    /// Excel-style TSV paste, validated completely before applying a single undo unit.
    pub fn paste(&mut self, doc: &mut Document, text: &str) -> Result<()> {
        ensure!(text.len() as u64 <= PAGE_BYTES, "Paste exceeds 1 MiB");
        if text.is_empty() {
            return self.replace(doc, "");
        }
        let page = self.page.as_ref().context("Page is loading")?;
        ensure!(
            page.version == doc.version(),
            "Page changed; wait for reload"
        );
        let source = Snapshot::from_bytes(text.as_bytes().to_vec());
        let dialect = Dialect::tsv();
        let mut changes = vec![];
        for (r, record) in RecordReader::new(&source, dialect, 0, source.len()).enumerate() {
            let row = page
                .rows
                .get(self.row + r)
                .context("Paste exceeds the current page; split into smaller blocks")?;
            for (c, field) in record.fields.iter().enumerate() {
                let value = String::from_utf8(unquote(field, &dialect))?;
                changes.push(self.change(row, self.col + c, &value)?);
            }
        }
        let at = changes.first().map_or(0, |c| c.range.start);
        for pair in changes.windows(2) {
            ensure!(
                pair[0].range.end <= pair[1].range.start,
                "Overlapping copybook fields cannot be pasted together"
            );
        }
        doc.apply_changes(changes, EditKind::Other, move |_| {
            SelectionSet::single(Selection::caret(at))
        });
        self.page = None;
        Ok(())
    }
    pub fn delete(&mut self, doc: &mut Document) -> Result<()> {
        let page = self.page.as_ref().context("Page is loading")?;
        ensure!(
            page.version == doc.version(),
            "Page changed; wait for reload"
        );
        let range = self.selected().context("No record selected")?.range.clone();
        apply(doc, range, vec![]);
        self.reset();
        Ok(())
    }
    pub fn append(&mut self, doc: &mut Document) -> Result<()> {
        ensure!(
            self.error.is_none(),
            "Resolve the table error before appending"
        );
        let page = self.page.as_ref().context("Page is loading")?;
        ensure!(
            page.version == doc.version(),
            "Page changed; wait for reload"
        );
        let snap = doc.snapshot();
        let len = snap.len();
        let mut bytes = vec![];
        match &self.mode {
            Mode::Delimited(d) => {
                let columns = self.selected().map_or(1, |r| r.fields.len());
                let eol = doc.eol().as_bytes();
                if len > 0 && snap.byte_at(len - 1) != Some(b'\n') {
                    bytes.extend_from_slice(eol);
                }
                yy_delimited::write_record(&vec![Vec::<u8>::new(); columns], d, eol, &mut bytes);
            }
            Mode::Fixed(spec) => {
                let stride = (spec.data_len + spec.separator.bytes().len()) as u64;
                ensure!(
                    len.is_multiple_of(stride) || (len > 0 && len % stride == spec.data_len as u64),
                    "Truncated fixed record; append refused"
                );
                if !len.is_multiple_of(stride) {
                    bytes.extend_from_slice(spec.separator.bytes());
                }
                let mut record = vec![spec.codec.charset.space(); spec.data_len];
                for field in &spec.layout.fields {
                    let mut issues = yy_cobol::Issues::default();
                    spec.codec.encode(
                        field,
                        yy_cobol::Input::Empty,
                        &mut record[field.offset..field.offset + field.len],
                        &mut issues,
                    );
                    ensure!(
                        issues.total() == 0,
                        "Cannot initialize field {}",
                        field.name
                    );
                }
                bytes.extend(record);
                bytes.extend_from_slice(spec.separator.bytes());
            }
        }
        apply(doc, len..len, bytes);
        self.reset();
        Ok(())
    }
}
fn apply(doc: &mut Document, range: Range<u64>, bytes: Vec<u8>) {
    let at = range.start;
    doc.apply_changes(
        vec![Change::replace_bytes(range, bytes)],
        EditKind::Other,
        move |_| SelectionSet::single(Selection::caret(at)),
    );
}
pub fn read_page(snap: &Snapshot, version: Version, mode: &Mode, start: u64) -> Result<Page> {
    let start = start.min(snap.len());
    let mut page = Page {
        version,
        rows: vec![],
        next: start,
        eof: start >= snap.len(),
        notice: String::new(),
    };
    match mode {
        Mode::Delimited(d) => {
            let end = (start + PAGE_BYTES).min(snap.len());
            for rec in RecordReader::new(snap, *d, start, end).take(PAGE_ROWS) {
                if rec.range.end == end && end < snap.len() && rec.terminator.is_empty() {
                    page.notice = if page.rows.is_empty() {
                        "Record exceeds the 1 MiB page budget; use text mode".into()
                    } else {
                        "Page byte budget reached; PgDn continues at the next record".into()
                    };
                    break;
                }
                ensure!(
                    rec.fields.len() <= 4096,
                    "More than 4096 columns; use text mode"
                );
                page.next = rec.range.end;
                page.rows.push(Row {
                    range: rec.range,
                    fields: rec.fields,
                });
            }
        }
        Mode::Fixed(spec) => {
            let sep = spec.separator.bytes();
            let stride = (spec.data_len + sep.len()) as u64;
            ensure!(
                stride <= PAGE_BYTES,
                "Record and separator exceed the 1 MiB page budget"
            );
            let mut at = start;
            for _ in 0..PAGE_ROWS.min((PAGE_BYTES / stride.max(1)) as usize) {
                if at == snap.len() {
                    break;
                }
                ensure!(
                    snap.len() - at >= spec.data_len as u64,
                    "Truncated record at byte {}",
                    at
                );
                let end = at + spec.data_len as u64;
                let has_sep = end + sep.len() as u64 <= snap.len();
                if has_sep {
                    ensure!(
                        snap.read(end..end + sep.len() as u64) == sep,
                        "Wrong record separator at byte {end}"
                    );
                } else {
                    ensure!(end == snap.len(), "Incomplete separator at byte {end}");
                }
                let next = if has_sep { at + stride } else { end };
                let fields = spec
                    .layout
                    .fields
                    .iter()
                    .map(|f| snap.read(at + f.offset as u64..at + (f.offset + f.len) as u64))
                    .collect();
                page.rows.push(Row {
                    range: at..next,
                    fields,
                });
                page.next = next;
                at = next;
            }
        }
    }
    page.eof = page.next >= snap.len();
    if page.rows.is_empty() && snap.is_empty() {
        if matches!(mode, Mode::Delimited(_)) {
            page.rows.push(Row {
                range: 0..0,
                fields: vec![vec![]],
            });
        } else {
            page.notice = "Empty file: F9 appends the first record".into();
        }
    }
    Ok(page)
}
pub fn fixed_spec(input: &str, base: &Path) -> Result<FixedSpec> {
    let parts = input.rsplitn(4, ';').collect::<Vec<_>>();
    ensure!(
        parts.len() == 4,
        "Use: COPYBOOK;RECORD_BYTES;CHARSET;SEPARATOR"
    );
    let sep = RecordSep::from_name(parts[0].trim()).context("Separator: none/crlf/lf/nl")?;
    let charset =
        yy_cobol::Charset::from_name(parts[1].trim()).context("Charset: MS932 or IBM-xxx")?;
    let length = parts[2]
        .trim()
        .parse::<usize>()
        .context("Invalid record length")?;
    ensure!(
        length > 0 && length as u64 <= PAGE_BYTES,
        "Record length: 1..1048576 bytes"
    );
    let path = base.join(parts[3].trim());
    ensure!(
        std::fs::metadata(&path)?.len() <= PAGE_BYTES,
        "Copybook exceeds 1 MiB"
    );
    let copybook = crate::editor::Editor::open(&path, None)?;
    let text = String::from_utf8(
        copybook
            .doc
            .snapshot()
            .read(0..copybook.doc.snapshot().len()),
    )?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("yyl"))
    {
        let def = yy_sheet::catalog::read_path(&path).map_err(anyhow::Error::msg)?;
        let mut spec = def
            .to_spec(
                yy_cobol::Codec {
                    charset,
                    little_endian: false,
                },
                sep,
            )
            .map_err(anyhow::Error::msg)?;
        ensure!(
            spec.layout.record_len <= length,
            "Layout exceeds declared record length"
        );
        if spec.is_multi() {
            ensure!(
                spec.data_len == length,
                "Declared length differs from .yyl length"
            );
        } else {
            spec.data_len = length;
        }
        return Ok(spec);
    }
    let mut spec = FixedSpec::new(
        &text,
        yy_cobol::Codec {
            charset,
            little_endian: false,
        },
        sep,
    )
    .map_err(anyhow::Error::msg)?;
    ensure!(
        spec.layout.record_len <= length,
        "Copybook is larger than the declared record length"
    );
    if spec.layout.fields.len() > 4096 {
        bail!("More than 4096 copybook fields");
    }
    spec.data_len = length;
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn csv(text: &str) -> (Document, Grid) {
        let doc = Document::from_text(text);
        let mut grid = Grid::new(Mode::Delimited(Dialect::csv()));
        grid.page = Some(read_page(doc.snapshot(), doc.version(), &grid.mode, 0).unwrap());
        (doc, grid)
    }
    fn reload(grid: &mut Grid, doc: &Document) {
        grid.page = Some(read_page(doc.snapshot(), doc.version(), &grid.mode, grid.start).unwrap());
    }
    #[test]
    fn csv_quotes_multiline_crlf_and_other_fields_survive() {
        let original = "id,note,unused\r\n1,\"first\r\nsecond\",\"unchanged\"\r\n2,last,tail";
        let (mut doc, mut grid) = csv(original);
        assert_eq!(grid.page.as_ref().unwrap().rows.len(), 3);
        grid.row = 1;
        grid.col = 1;
        assert_eq!(grid.value().unwrap(), "first\r\nsecond");
        grid.replace(&mut doc, "new,\"quoted\"\nline").unwrap();
        let expected =
            "id,note,unused\r\n1,\"new,\"\"quoted\"\"\nline\",\"unchanged\"\r\n2,last,tail";
        assert_eq!(
            doc.snapshot().read(0..doc.snapshot().len()),
            expected.as_bytes()
        );
        doc.undo();
        assert_eq!(
            doc.snapshot().read(0..doc.snapshot().len()),
            original.as_bytes()
        );
    }
    #[test]
    fn rectangle_paste_is_one_undo_and_validates_before_changing() {
        let (mut doc, mut grid) = csv("a,b,c\r\nd,e,f\r\n");
        grid.col = 1;
        grid.paste(&mut doc, "日本語\t1\r\n\"multi\nline\"\t2")
            .unwrap();
        assert_eq!(
            doc.snapshot().read(0..doc.snapshot().len()),
            "a,日本語,1\r\nd,\"multi\nline\",2\r\n".as_bytes()
        );
        doc.undo();
        reload(&mut grid, &doc);
        let before = doc.version();
        assert!(grid.paste(&mut doc, "one\ttwo\tthree").is_err());
        assert_eq!(doc.version(), before);
        assert_eq!(
            doc.snapshot().read(0..doc.snapshot().len()),
            b"a,b,c\r\nd,e,f\r\n"
        );
    }
    #[test]
    fn paging_respects_quoted_newlines_and_does_not_edit_partial_records() {
        let text = (0..300)
            .map(|n| format!("{n},\"one\ntwo\"\n"))
            .collect::<String>();
        let (doc, mut grid) = csv(&text);
        assert_eq!(grid.page.as_ref().unwrap().rows.len(), PAGE_ROWS);
        assert!(grid.next_page());
        reload(&mut grid, &doc);
        assert_eq!(grid.base, 128);
        assert_eq!(grid.value().unwrap(), "128");
        assert!(grid.previous_page());
        reload(&mut grid, &doc);
        assert_eq!(grid.value().unwrap(), "0");
        let text = format!("header\n\"{}\"\n", "x".repeat(PAGE_BYTES as usize));
        let (_, grid) = csv(&text);
        assert_eq!(grid.page.as_ref().unwrap().rows.len(), 1);
        assert!(!grid.page.as_ref().unwrap().notice.is_empty());
    }
    #[test]
    fn record_ending_exactly_at_page_budget_remains_editable() {
        let text = format!("{}\nb\n", "a".repeat(PAGE_BYTES as usize - 1));
        let (doc, mut grid) = csv(&text);
        assert_eq!(grid.page.as_ref().unwrap().rows.len(), 1);
        assert_eq!(grid.page.as_ref().unwrap().next, PAGE_BYTES);
        assert!(grid.next_page());
        reload(&mut grid, &doc);
        assert_eq!(grid.value().unwrap(), "b");
    }
    #[test]
    fn append_delete_and_empty_records() {
        let (mut doc, mut grid) = csv("a,b");
        grid.append(&mut doc).unwrap();
        assert_eq!(
            doc.snapshot().read(0..doc.snapshot().len()),
            format!(
                "a,b{0},{0}",
                doc.eol()
                    .as_bytes()
                    .iter()
                    .map(|&b| char::from(b))
                    .collect::<String>()
            )
            .as_bytes()
        );
        doc.undo();
        reload(&mut grid, &doc);
        grid.delete(&mut doc).unwrap();
        assert!(doc.snapshot().is_empty());
        reload(&mut grid, &doc);
        grid.replace(&mut doc, "").unwrap();
        assert_eq!(doc.snapshot().read(0..2), b"\"\"");
    }
    fn spec() -> FixedSpec {
        let copybook =
            "01 REC.\n 05 ID PIC X(4).\n 05 AMOUNT PIC S9(5) COMP-3.\n 05 NAME PIC X(4).\n";
        let mut spec = FixedSpec::new(
            copybook,
            yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            RecordSep::None,
        )
        .unwrap();
        assert_eq!(spec.data_len, 11);
        spec.data_len = 13;
        spec
    }
    #[test]
    fn packed_fields_and_opaque_padding_are_preserved_on_disk() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("host.dat");
        let bytes = [
            b'A', b'B', b'C', b'D', 0x12, 0x34, 0x5c, b'c', b'a', b't', b' ', 0xde, 0xad,
        ];
        std::fs::write(&path, bytes)?;
        let mut editor = crate::editor::Editor::open_raw(&path)?;
        let mut grid = Grid::new(Mode::Fixed(Arc::new(spec())));
        reload(&mut grid, &editor.doc);
        grid.col = 1;
        assert_eq!(grid.value()?, "12345");
        let version = editor.doc.version();
        assert!(grid.replace(&mut editor.doc, "12.3").is_err());
        assert_eq!(editor.doc.version(), version);
        assert!(grid.replace(&mut editor.doc, "123456").is_err());
        grid.col = 0;
        assert!(grid.replace(&mut editor.doc, "too long").is_err());
        grid.replace(&mut editor.doc, "Q")?;
        let out = dir.path().join("out");
        yy_io::save_snapshot(
            editor.doc.snapshot(),
            &out,
            &editor.doc.save_format(editor.doc.encoding(), false),
            None,
        )?;
        let result = std::fs::read(out)?;
        assert_eq!(&result[..4], b"Q   ");
        assert_eq!(&result[4..], &bytes[4..]);
        editor.doc.undo();
        assert_eq!(editor.doc.snapshot().read(0..13), bytes);
        Ok(())
    }
    #[test]
    fn fixed_separators_and_truncated_records_are_checked() {
        let mut spec = spec();
        spec.separator = RecordSep::Crlf;
        let mode = Mode::Fixed(Arc::new(spec));
        let doc = Document::from_text("ABCD123cat   \r\n");
        assert!(read_page(doc.snapshot(), doc.version(), &mode, 0).is_ok());
        let doc = Document::from_text("too short");
        assert!(read_page(doc.snapshot(), doc.version(), &mode, 0).is_err());
        let doc = Document::from_text("ABCD123cat   XX");
        assert!(read_page(doc.snapshot(), doc.version(), &mode, 0).is_err());
    }
}
