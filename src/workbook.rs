//! The actual yysheet workbook engine, kept on owned disk storage.
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use yy_sheet::{Document, Sheet, Value, Workbook, style::Rect};

pub struct Book {
    pub doc: Document,
    pub sheet: usize,
    pub row: u64,
    pub col: u32,
    pub anchor: Option<(u64, u32)>,
    pub top: u64,
    pub left: u32,
    pub path: Option<PathBuf>,
    pub digest: Option<[u8; 32]>,
    _storage: tempfile::TempDir,
}
#[derive(Clone)]
pub enum Op {
    Set(String),
    Paste(String),
    Clear,
    InsertRows,
    DeleteRows,
    InsertCols,
    DeleteCols,
    Sort(bool),
    Filter(String),
    ClearView,
    Fill(u64),
    Width(u16),
    Bold,
    Format(String),
    MoveColumn(u32),
    NextSheet,
    AddSheet(String),
    Save(PathBuf),
    Export(PathBuf),
    RowLayout(String),
    Height(u16),
    ExportFixed(PathBuf),
}
fn io(error: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(error.to_string())
}
impl Book {
    pub fn empty() -> Result<Self> {
        let storage = tempfile::tempdir()?;
        let ctx = yy_sheet::Context::new(256 * 1024 * 1024, storage.path().to_owned());
        Ok(Self {
            doc: Document::new(ctx),
            sheet: 0,
            row: 0,
            col: 0,
            anchor: None,
            top: 0,
            left: 0,
            path: None,
            digest: None,
            _storage: storage,
        })
    }
    pub fn open(path: &Path) -> Result<Self> {
        let mut book = Self::empty()?;
        let copy = book._storage.path().join("source.yys");
        std::fs::copy(path, &copy)?;
        let hash = crate::editor::digest(&copy)?;
        ensure!(
            hash == crate::editor::digest(path)?,
            "Workbook changed while opening"
        );
        book.doc = yy_sheet::yys::open(book.doc.ctx.clone(), &copy)?;
        ensure!(!book.doc.book.sheets.is_empty(), "Workbook has no sheets");
        book.path = Some(path.canonicalize()?);
        book.digest = Some(hash);
        Ok(book)
    }
    pub fn import(
        snapshot: &yy_buffer::Snapshot,
        mode: Option<&crate::data::Mode>,
    ) -> Result<Self> {
        let mut book = Self::empty()?;
        let input = book._storage.path().join("import");
        let format = yy_io::SaveFormat::utf8();
        yy_io::save_snapshot(snapshot, &input, &format, None)?;
        let sheet = match mode {
            Some(crate::data::Mode::Fixed(spec)) => {
                let (sheet, report) = if spec.is_multi() {
                    yy_sheet::fixed::import_multi(&book.doc.ctx, &input, spec, &|_, _| true)?
                } else {
                    if spec.data_len != spec.layout.record_len {
                        let copybook = format!(
                            "{}\n 05 FILLER PIC X({}).\n",
                            spec.copybook,
                            spec.data_len - spec.layout.record_len
                        );
                        let padded =
                            yy_sheet::fixed::FixedSpec::new(&copybook, spec.codec, spec.separator)
                                .map_err(anyhow::Error::msg)?;
                        yy_sheet::fixed::import(&book.doc.ctx, &input, &padded, &|_, _| true)?
                    } else {
                        yy_sheet::fixed::import(&book.doc.ctx, &input, spec, &|_, _| true)?
                    }
                };
                ensure!(
                    report.remainder == 0,
                    "Truncated fixed records cannot be imported"
                );
                sheet
            }
            _ => {
                let dialect = match mode {
                    Some(crate::data::Mode::Delimited(d)) => *d,
                    _ => yy_delimited::Dialect::csv(),
                };
                let mut opts = yy_sheet::csv::preview_as(
                    &input,
                    Some(dialect),
                    None,
                    Some(yy_core::Encoding::Utf8),
                )?
                .options;
                opts.header = true;
                yy_sheet::csv::import(&book.doc.ctx, &input, &opts, &|_, _| true)?
            }
        };
        let mut sheet = sheet;
        if let Some(crate::data::Mode::Fixed(spec)) = mode {
            attach_raw(&book.doc.ctx, &mut sheet, snapshot, spec)?;
        }
        book.doc.book.sheets = vec![sheet];
        book.doc.dirty = true;
        Ok(book)
    }
    pub fn visible_sheets(&self) -> Vec<usize> {
        (0..self.doc.book.sheets.len())
            .filter(|i| Some(*i) != meta_index(&self.doc.book))
            .collect()
    }
    pub fn row_height(&self, row: u64) -> u16 {
        let source = self.current().source_row(row);
        meta_index(&self.doc.book)
            .and_then(|i| {
                self.doc.book.sheets[i]
                    .cells
                    .get(&(source + 1, self.sheet as u32))
            })
            .and_then(Value::as_number)
            .unwrap_or(1.0)
            .clamp(1.0, 20.0) as u16
    }
    pub fn data_cols(&self) -> u32 {
        let s = self.current();
        raw_column(s).unwrap_or_else(|| s.extent().1)
    }

    pub fn current(&self) -> &Sheet {
        &self.doc.book.sheets[self.sheet]
    }
    pub fn range(&self) -> Rect {
        let (r, c) = self.anchor.unwrap_or((self.row, self.col));
        Rect::new(r, c, self.row, self.col)
    }
    pub fn value(&self, r: u64, c: u32) -> Result<Value> {
        Ok(self.current().get(&self.doc.ctx, r, c)?)
    }
    pub fn entry(&self) -> Result<String> {
        if let Some(f) = self
            .current()
            .formula_at(self.current().source_row(self.row), self.col)
        {
            return Ok(f.text.to_string());
        }
        Ok(self.value(self.row, self.col)?.general_text())
    }
    pub fn copy(&self) -> Result<String> {
        let rect = self.range();
        ensure!(
            (rect.bottom - rect.top + 1).saturating_mul(u64::from(rect.right - rect.left + 1))
                <= 100_000,
            "Copy exceeds 100,000 cells"
        );
        let d = yy_delimited::Dialect::tsv();
        let mut out = vec![];
        for r in rect.top..=rect.bottom {
            let values = (rect.left..=rect.right)
                .map(|c| self.value(r, c).map(|v| v.general_text()))
                .collect::<Result<Vec<_>>>()?;
            yy_delimited::write_record(
                &values,
                &d,
                if r < rect.bottom { b"\n" } else { b"" },
                &mut out,
            );
            ensure!(out.len() <= 1024 * 1024, "Copy exceeds 1 MiB");
        }
        Ok(String::from_utf8(out)?)
    }
    fn put(
        sheet: &mut Sheet,
        ctx: &yy_sheet::Context,
        r: u64,
        c: u32,
        text: &str,
    ) -> std::io::Result<()> {
        if raw_column(sheet) == Some(c) {
            return Err(io("Original-record metadata is read-only"));
        }
        if yy_sheet::fixed::layout_column(sheet) == Some(c) && r != 0 {
            return yy_sheet::fixed::set_row_layout(ctx, sheet, r, text).map_err(io);
        }
        if text.starts_with('=') {
            return sheet.set_formula(ctx, r, c, text).map_err(io);
        }
        let value = if let Some(text) = text.strip_prefix('\'') {
            Value::text(text)
        } else if text.is_empty() {
            Value::Empty
        } else if let Ok(number) = text.parse::<f64>() {
            if number.is_finite() {
                Value::Number(number)
            } else {
                Value::text(text)
            }
        } else if text.eq_ignore_ascii_case("true") {
            Value::Bool(true)
        } else if text.eq_ignore_ascii_case("false") {
            Value::Bool(false)
        } else {
            Value::text(text)
        };
        if let Some((spec, field)) = yy_sheet::fixed::field_at(ctx, sheet, r, c) {
            if let Some((_, scale)) = field.kind.digits_scale()
                && let Some(n) = yy_cobol::Decimal::parse(text.strip_prefix('\'').unwrap_or(text))
                && n.truncate(scale).and_then(|v| v.rescale(n.scale)) != Some(n)
            {
                return Err(io("Fractional digits would be lost"));
            }
            let fitted = yy_sheet::fixed::entry_value(spec, field, text).map_err(io)?;
            return sheet.set(ctx, r, c, fitted);
        }
        sheet.set(ctx, r, c, value)
    }
    pub fn apply(&mut self, op: Op) -> Result<()> {
        let si = self.sheet;
        let rect = self.range();
        let row = self.row;
        let col = self.col;
        match op {
            Op::Save(path) => return self.save(&path),
            Op::Export(path) => return self.export(&path),
            Op::ExportFixed(path) => return crate::fixed_export::export(self, &path),
            Op::NextSheet => {
                let indices = self.visible_sheets();
                let at = indices.iter().position(|i| *i == si).unwrap_or(0);
                self.sheet = indices[(at + 1) % indices.len()];
                self.row = 0;
                self.col = 0;
                self.anchor = None;
                return Ok(());
            }
            _ => {}
        }
        if matches!(
            op,
            Op::InsertRows | Op::DeleteRows | Op::InsertCols | Op::DeleteCols | Op::MoveColumn(_)
        ) {
            ensure!(
                self.current().view.rows.is_none(),
                "Clear sorting/filtering before structural edits"
            );
        }
        if let Some(raw) = raw_column(self.current()) {
            ensure!(
                !matches!(op, Op::InsertCols | Op::DeleteCols) || rect.right < raw,
                "Original-record metadata cannot be removed or shifted outside the fixed table"
            );
            ensure!(
                !matches!(op, Op::Fill(_)) || rect.right < raw,
                "Cannot autofill original-record metadata"
            );
            ensure!(
                !matches!(op, Op::MoveColumn(to) if to >= raw || col >= raw),
                "Cannot move original-record metadata"
            );
        }
        self.doc.edit(move |book, ctx| {
            match op {
                Op::Set(text) => Self::put(&mut book.sheets[si], ctx, row, col, &text)?,
                Op::Paste(text) => {
                    if text.len() > 1024 * 1024 {
                        return Err(io("Paste exceeds 1 MiB"));
                    }
                    let snap = yy_buffer::Snapshot::from_bytes(text.into_bytes());
                    let d = yy_delimited::Dialect::tsv();
                    let mut count = 0;
                    for (r, rec) in
                        yy_delimited::RecordReader::new(&snap, d, 0, snap.len()).enumerate()
                    {
                        for (c, raw) in rec.fields.iter().enumerate() {
                            count += 1;
                            if count > 100_000 {
                                return Err(io("Paste exceeds 100,000 cells"));
                            }
                            let value =
                                String::from_utf8(yy_delimited::unquote(raw, &d)).map_err(io)?;
                            Self::put(
                                &mut book.sheets[si],
                                ctx,
                                row + r as u64,
                                col + c as u32,
                                &value,
                            )?;
                        }
                    }
                }
                Op::Clear => {
                    if (rect.bottom - rect.top + 1)
                        .saturating_mul(u64::from(rect.right - rect.left + 1))
                        > 100_000
                    {
                        return Err(io("Clear exceeds 100,000 cells"));
                    }
                    for r in rect.top..=rect.bottom {
                        for c in rect.left..=rect.right {
                            if raw_column(&book.sheets[si]) != Some(c) {
                                book.sheets[si].set(ctx, r, c, Value::Empty)?;
                            }
                        }
                    }
                }
                Op::InsertRows => {
                    shift_heights(book, si, rect.top, rect.bottom - rect.top + 1, false);
                    book.edit_rows_cols(
                        ctx,
                        si,
                        yy_formula::Edit::InsertRows(rect.top, rect.bottom - rect.top + 1),
                    )?;
                }
                Op::DeleteRows => {
                    shift_heights(book, si, rect.top, rect.bottom - rect.top + 1, true);
                    book.edit_rows_cols(
                        ctx,
                        si,
                        yy_formula::Edit::DeleteRows(rect.top, rect.bottom - rect.top + 1),
                    )?;
                }
                Op::InsertCols => book.edit_rows_cols(
                    ctx,
                    si,
                    yy_formula::Edit::InsertCols(rect.left, rect.right - rect.left + 1),
                )?,
                Op::DeleteCols => book.edit_rows_cols(
                    ctx,
                    si,
                    yy_formula::Edit::DeleteCols(rect.left, rect.right - rect.left + 1),
                )?,
                Op::Sort(desc) => {
                    let sheet = &mut book.sheets[si];
                    sheet.absorb_free_cells(ctx, col)?;
                    sheet.view.sort = vec![yy_sheet::query::SortKey { col, desc }];
                    let table = sheet.query_table(ctx)?;
                    yy_sheet::query::apply(ctx, &table, &mut sheet.view)?;
                }
                Op::Filter(pattern) => {
                    let sheet = &mut book.sheets[si];
                    sheet.absorb_free_cells(ctx, col)?;
                    sheet.view.filters.retain(|f| f.col != col);
                    sheet.view.filters.push(yy_sheet::query::ColFilter {
                        col,
                        cond: yy_sheet::query::Cond::Text {
                            op: yy_sheet::query::TextOp::Wildcard,
                            pattern,
                            negate: false,
                        },
                    });
                    let table = sheet.query_table(ctx)?;
                    yy_sheet::query::apply(ctx, &table, &mut sheet.view)?;
                }
                Op::ClearView => book.sheets[si].view = yy_sheet::View::default(),
                Op::Fill(n) => {
                    if n == 0 || n > 1_000_000 {
                        return Err(io("Fill count: 1..1000000"));
                    }
                    let src = (rect.top, rect.left, rect.bottom, rect.right);
                    let dst = (rect.top, rect.left, rect.bottom + n, rect.right);
                    let opts = yy_sheet::autofill::FillOptions::auto(false, book.date_system);
                    book.sheets[si].autofill(ctx, src, dst, &opts).map_err(io)?;
                }
                Op::Height(height) => {
                    if !(1..=20).contains(&height) {
                        return Err(io("Height: 1..20 terminal lines"));
                    }
                    let mi = if let Some(i) = meta_index(book) {
                        i
                    } else {
                        let mut meta = Sheet::new(VIEW_META);
                        meta.set(ctx, 0, 0, Value::text(VIEW_MARKER))?;
                        book.sheets.push(meta);
                        book.sheets.len() - 1
                    };
                    if rect.bottom - rect.top > 100_000 {
                        return Err(io("Set at most 100,000 row heights at once"));
                    }
                    for r in rect.top..=rect.bottom {
                        let source = book.sheets[si].source_row(r);
                        book.sheets[mi].set(
                            ctx,
                            source + 1,
                            si as u32,
                            Value::Number(height as f64),
                        )?;
                    }
                }
                Op::Width(width) => {
                    if !(4..=80).contains(&width) {
                        return Err(io("Width: 4..80 terminal cells"));
                    }
                    for c in rect.left..=rect.right {
                        Arc::make_mut(&mut book.sheets[si].col_widths).insert(c, width as f32);
                    }
                }
                Op::Bold => {
                    let sheet = &mut book.sheets[si];
                    let bold = !sheet.style_at(row, col).bold.unwrap_or(false);
                    sheet.styles.set(
                        rect,
                        yy_sheet::style::Style {
                            bold: Some(bold),
                            ..Default::default()
                        },
                    );
                }
                Op::Format(format) => book.sheets[si].styles.set(
                    rect,
                    yy_sheet::style::Style {
                        num_fmt: Some(format.into()),
                        ..Default::default()
                    },
                ),
                Op::AddSheet(name) => {
                    if name == VIEW_META
                        || name.is_empty()
                        || book
                            .sheets
                            .iter()
                            .any(|s| s.name.eq_ignore_ascii_case(&name))
                    {
                        return Err(io("Sheet name must be unique and nonempty"));
                    }
                    book.sheets.push(Sheet::new(&name));
                }
                Op::MoveColumn(to) => move_column(book, si, col, to)?,
                Op::RowLayout(name) => {
                    yy_sheet::fixed::set_row_layout(ctx, &mut book.sheets[si], row, &name)
                        .map_err(io)?;
                }
                Op::NextSheet | Op::Save(_) | Op::Export(_) | Op::ExportFixed(_) => unreachable!(),
            }
            Ok(())
        })?;
        self.anchor = None;
        Ok(())
    }
    pub fn save(&mut self, path: &Path) -> Result<()> {
        ensure!(
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("yys")),
            "Save workbooks as .yys; use Export CSV for text data"
        );
        let path = if path.exists() {
            path.canonicalize()?
        } else {
            path.parent()
                .unwrap_or(Path::new("."))
                .canonicalize()?
                .join(path.file_name().context("Missing filename")?)
        };
        let same = self.path.as_ref() == Some(&path);
        if same {
            ensure!(
                Some(crate::editor::digest(&path)?) == self.digest,
                "Workbook changed externally"
            );
        } else {
            ensure!(!path.exists(), "Save as refuses to overwrite another file");
        }
        // save_book relocates live chunks to its output. Keep that output private so
        // its writable Store never holds a user's destination file open on Windows.
        let private = tempfile::NamedTempFile::new_in(self._storage.path())?
            .into_temp_path()
            .keep()?;
        let saved = yy_sheet::yys::save_book(
            &self.doc.book,
            yy_sheet::yys::SaveState {
                file: None,
                last_dir: 0,
            },
            &private,
            &mut |_, _| true,
        )?;
        drop(saved);
        let temporary =
            tempfile::NamedTempFile::new_in(path.parent().context("No parent")?)?.into_temp_path();
        std::fs::copy(&private, &temporary)?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)?
            .sync_all()?;
        if same {
            ensure!(
                Some(crate::editor::digest(&path)?) == self.digest,
                "Workbook changed during save"
            );
            temporary.persist(&path).map_err(|e| e.error)?;
        } else {
            temporary.persist_noclobber(&path).map_err(|e| e.error)?;
        }
        self.digest = Some(crate::editor::digest(&path)?);
        self.path = Some(path);
        self.doc.dirty = false;
        Ok(())
    }
    pub fn export(&self, path: &Path) -> Result<()> {
        ensure!(
            !path.exists(),
            "Export refuses to overwrite an existing file"
        );
        let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?
            .into_temp_path();
        let opts = yy_sheet::csv::ExportOptions::default();
        let mut sheet = self.current().clone();
        if let Some(c) = raw_column(&sheet) {
            Arc::make_mut(&mut sheet.table.columns).remove(c as usize);
        }
        let report = yy_sheet::csv::export(
            &self.doc.ctx,
            &sheet,
            self.doc.book.date_system,
            &temporary,
            &opts,
            self.current().view.rows.as_deref().map(|v| v.as_slice()),
            &|_, _| true,
        )?;
        ensure!(report.unencodable == 0, "Unencodable values in export");
        temporary.persist_noclobber(path).map_err(|e| e.error)?;
        Ok(())
    }
}
/// Move complete columns; noncontiguous formula references are rejected atomically.
fn move_column(book: &mut Workbook, si: usize, from: u32, to: u32) -> std::io::Result<()> {
    let sheet = &book.sheets[si];
    let cols = sheet.extent().1;
    if from >= cols || to >= cols {
        return Err(io("Column outside the sheet"));
    }
    if from == to {
        return Ok(());
    }
    // Reordering stored data is safe only when no formulas/styles would become stale.
    if book.sheets.iter().any(|s| !s.formulas.is_empty()) || !sheet.styles.layers().is_empty() {
        return Err(io(
            "Column moves with formulas or styles require preserving their references; clear them or use insert/delete instead",
        ));
    }
    let map = |c: u32| {
        if c == from {
            to
        } else if from < to && c > from && c <= to {
            c - 1
        } else if from > to && c >= to && c < from {
            c + 1
        } else {
            c
        }
    };
    let s = &mut book.sheets[si];
    if from >= s.table.cols() || to >= s.table.cols() {
        return Err(io("Move columns within the populated table"));
    }
    let columns = Arc::make_mut(&mut s.table.columns);
    let moved = columns.remove(from as usize);
    columns.insert(to as usize, moved);
    s.cells = Arc::new(
        s.cells
            .iter()
            .map(|(&(r, c), v)| ((r, map(c)), v.clone()))
            .collect(),
    );
    s.col_widths = Arc::new(s.col_widths.iter().map(|(&c, &w)| (map(c), w)).collect());
    s.view.remap_cols(|c| Some(map(c)));
    Ok(())
}

pub const RAW_COLUMN: &str = "__YYTE_ORIGINAL_RECORD";
const VIEW_META: &str = "__yyte_view_v1";
const VIEW_MARKER: &str = "YYTE_ROW_HEIGHTS_V1";
fn meta_index(book: &Workbook) -> Option<usize> {
    book.sheets.iter().position(|s| {
        s.name.as_ref() == VIEW_META && s.cells.get(&(0, 0)) == Some(&Value::text(VIEW_MARKER))
    })
}
pub fn raw_column(sheet: &Sheet) -> Option<u32> {
    sheet.fixed.as_ref()?;
    sheet
        .table
        .columns
        .iter()
        .position(|c| c.name.as_ref() == RAW_COLUMN && c.field.is_none())
        .map(|i| i as u32)
}
fn attach_raw(
    ctx: &yy_sheet::Context,
    sheet: &mut Sheet,
    snapshot: &yy_buffer::Snapshot,
    spec: &yy_sheet::fixed::FixedSpec,
) -> Result<()> {
    ensure!(
        !sheet
            .table
            .columns
            .iter()
            .any(|c| c.name.as_ref() == RAW_COLUMN),
        "Reserved metadata column name"
    );
    let mut pieces = vec![];
    let stride = (spec.data_len + spec.separator.bytes().len()) as u64;
    let per_chunk =
        (4 * 1024 * 1024 / (spec.data_len.saturating_mul(2).saturating_add(8))).clamp(1, 4096);
    let mut values = vec![];
    let mut at = 0;
    while at < snapshot.len() {
        let end = (at + stride).min(snapshot.len());
        ensure!(end - at >= spec.data_len as u64, "Truncated record");
        values.push(Value::text(&yy_cobol::hex_text(&snapshot.read(at..end))));
        at = end;
        if values.len() >= per_chunk || at == snapshot.len() {
            let chunk = yy_sheet::chunk::Chunk::create(
                ctx,
                yy_sheet::chunk::Data::from_values(values.iter().map(yy_sheet::chunk::CellRef::of)),
            )?;
            pieces.push(yy_sheet::column::Piece {
                start: 0,
                len: chunk.rows,
                chunk,
            });
            values.clear();
        }
    }
    let column = yy_sheet::Column::from_pieces(RAW_COLUMN, pieces);
    ensure!(
        column.rows() == sheet.table.rows,
        "Original-record metadata count mismatch"
    );
    Arc::make_mut(&mut sheet.table.columns).push(column);
    Ok(())
}

fn shift_heights(book: &mut Workbook, si: usize, at: u64, count: u64, delete: bool) {
    let Some(mi) = meta_index(book) else {
        return;
    };
    book.sheets[mi].cells = Arc::new(
        book.sheets[mi]
            .cells
            .iter()
            .filter_map(|(&(r, c), v)| {
                if r == 0 || c != si as u32 || r <= at {
                    return Some(((r, c), v.clone()));
                }
                if delete && r <= at + count {
                    return None;
                }
                Some(((if delete { r - count } else { r + count }, c), v.clone()))
            })
            .collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn csv() -> Result<Book> {
        Book::import(
            &yy_buffer::Snapshot::from_bytes(b"name,amount\r\nbeta,2\r\nalpha,1\r\n"),
            None,
        )
    }
    #[test]
    fn formulas_fill_references_and_atomic_paste() -> Result<()> {
        let mut b = Book::empty()?;
        b.apply(Op::Paste("1\t=A1*2\n2\t=A2*2".into()))?;
        assert_eq!(b.value(1, 1)?.as_number(), Some(4.0));
        b.col = 1;
        b.row = 0;
        b.apply(Op::Fill(3))?;
        assert_eq!(b.value(1, 1)?.as_number(), Some(4.0));
        assert_eq!(b.current().formula_at(3, 1).unwrap().text.as_ref(), "=A4*2");
        b.row = 0;
        b.col = 0;
        b.apply(Op::InsertRows)?;
        assert_eq!(b.current().formula_at(1, 1).unwrap().text.as_ref(), "=A2*2");
        assert_eq!(b.value(1, 1)?.as_number(), Some(2.0));
        b.doc.undo();
        assert_eq!(b.value(0, 1)?.as_number(), Some(2.0));
        b.doc.redo();
        assert_eq!(b.value(1, 1)?.as_number(), Some(2.0));
        Ok(())
    }
    #[test]
    fn sorting_filtering_column_moves_and_structural_edits() -> Result<()> {
        let mut b = csv()?;
        b.col = 0;
        b.apply(Op::Sort(false))?;
        assert_eq!(b.value(1, 0)?.general_text(), "alpha");
        b.apply(Op::Filter("beta".into()))?;
        assert_eq!(b.current().visible_rows(), 1);
        assert_eq!(b.value(1, 0)?.general_text(), "beta");
        assert!(b.apply(Op::InsertRows).is_err());
        b.apply(Op::ClearView)?;
        b.apply(Op::MoveColumn(1))?;
        assert_eq!(b.value(1, 0)?.as_number(), Some(2.0));
        b.row = 1;
        b.col = 0;
        b.apply(Op::DeleteRows)?;
        assert_eq!(b.value(1, 1)?.general_text(), "alpha");
        b.apply(Op::InsertCols)?;
        assert_eq!(b.value(1, 2)?.general_text(), "alpha");
        Ok(())
    }
    #[test]
    fn sizes_styles_and_sheets_survive_save_reload_and_undo() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("book.yys");
        let mut b = csv()?;
        b.row = 1;
        b.col = 1;
        b.apply(Op::Height(3))?;
        b.apply(Op::Width(28))?;
        b.apply(Op::Bold)?;
        b.apply(Op::Format("0.00".into()))?;
        b.apply(Op::AddSheet("second".into()))?;
        assert_eq!(b.visible_sheets().len(), 2);
        b.save(&path)?;
        let mut b = Book::open(&path)?;
        assert_eq!(b.row_height(1), 3);
        assert_eq!(b.current().col_widths.get(&1), Some(&28.0));
        assert_eq!(b.current().style_at(1, 1).bold, Some(true));
        assert_eq!(b.current().style_at(1, 1).num_fmt.as_deref(), Some("0.00"));
        b.row = 1;
        b.apply(Op::InsertRows)?;
        assert_eq!(b.row_height(2), 3);
        assert_eq!(b.row_height(1), 1);
        b.doc.undo();
        assert_eq!(b.row_height(1), 3);
        b.apply(Op::NextSheet)?;
        assert_eq!(b.current().name.as_ref(), "second");
        b.apply(Op::NextSheet)?;
        assert_eq!(b.sheet, 0);
        std::fs::write(&path, b"external")?;
        assert!(b.save(&path).is_err());
        Ok(())
    }
    pub fn fixed(bytes: &[u8], padding: bool) -> Result<Book> {
        let copybook = "01 REC.\n 05 NAME PIC X(4).\n 05 AMOUNT PIC S9(5) COMP-3.\n";
        let spec = yy_sheet::fixed::FixedSpec::new(
            copybook,
            yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            yy_sheet::fixed::RecordSep::None,
        )
        .map_err(anyhow::Error::msg)?;
        let mut spec = spec;
        if padding {
            spec.data_len = 9;
        }
        Book::import(
            &yy_buffer::Snapshot::from_bytes(bytes),
            Some(&crate::data::Mode::Fixed(Arc::new(spec))),
        )
    }
    #[test]
    fn fixed_export_preserves_original_padding_packed_bytes_and_roundtrip() -> Result<()> {
        let bytes = [b'A', b'B', b' ', b' ', 0x12, 0x34, 0x5f, 0xde, 0xad];
        let mut b = fixed(&bytes, true)?;
        let dir = tempfile::tempdir()?;
        let original = dir.path().join("original.dat");
        crate::fixed_export::export(&b, &original)?;
        assert_eq!(std::fs::read(&original)?, bytes);
        let yys = dir.path().join("book.yys");
        b.save(&yys)?;
        let mut b = Book::open(&yys)?;
        let header = u64::from(b.current().table.header);
        b.row = header;
        b.col = 0;
        b.apply(Op::Set("Q".into()))?;
        let output = dir.path().join("edited.dat");
        crate::fixed_export::export(&b, &output)?;
        let out = std::fs::read(&output)?;
        assert_eq!(&out[..4], b"Q   ");
        assert_eq!(&out[4..], &bytes[4..]);
        b.col = 1;
        assert!(b.apply(Op::Set("12.3".into())).is_err());
        let row = b.row;
        b.doc
            .edit(|book, ctx| book.sheets[0].set(ctx, row, 1, Value::Number(12.3)))?;
        let bad = dir.path().join("bad.dat");
        assert!(crate::fixed_export::export(&b, &bad).is_err());
        assert!(!bad.exists());
        assert!(crate::fixed_export::export(&b, &output).is_err());
        Ok(())
    }
    #[test]
    fn fixed_multilayout_export_uses_each_rows_layout_and_sorted_original_bytes() -> Result<()> {
        let defs = vec![
            (
                "A".into(),
                "01 R.\n 05 TAG PIC X VALUE 'A'.\n 05 TEXT PIC X(3).".into(),
            ),
            (
                "B".into(),
                "01 R.\n 05 TAG PIC X VALUE 'B'.\n 05 COUNT PIC 9(3).".into(),
            ),
        ];
        let spec = yy_sheet::fixed::FixedSpec::new_multi(
            &defs,
            4,
            yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            yy_sheet::fixed::RecordSep::Crlf,
        )
        .map_err(anyhow::Error::msg)?;
        let input = b"Axyz\r\nB012";
        let mut b = Book::import(
            &yy_buffer::Snapshot::from_bytes(input),
            Some(&crate::data::Mode::Fixed(Arc::new(spec))),
        )?;
        let dir = tempfile::tempdir()?;
        let out = dir.path().join("multi.dat");
        crate::fixed_export::export(&b, &out)?;
        assert_eq!(std::fs::read(&out)?, input);
        b.row = 2;
        b.apply(Op::RowLayout("B".into()))?;
        b.col = 2;
        b.apply(Op::Set("42".into()))?;
        let edited = dir.path().join("edited.dat");
        crate::fixed_export::export(&b, &edited)?;
        assert_eq!(std::fs::read(edited)?, b"Axyz\r\nB042");
        b.col = 1;
        b.apply(Op::Sort(true))?;
        let sorted = dir.path().join("sorted.dat");
        crate::fixed_export::export(&b, &sorted)?;
        assert_eq!(std::fs::read(sorted)?, b"B042\r\nAxyz\r\n");
        Ok(())
    }
    #[test]
    fn fixed_paste_failure_rolls_back_the_entire_rectangle() -> Result<()> {
        let bytes = [b'A', b'B', b' ', b' ', 0x12, 0x34, 0x5c];
        let mut b = fixed(&bytes, false)?;
        b.row = u64::from(b.current().table.header);
        let old = b.value(b.row, 0)?;
        assert!(b.apply(Op::Paste("Q\t100000".into())).is_err());
        assert_eq!(b.value(b.row, 0)?, old);
        Ok(())
    }
}
