//! Fixed-layout export overlays edited fields onto the original record bytes.
use anyhow::{Context, Result, ensure};
use std::{io::Write, path::Path};
use yy_sheet::Value;
fn unchanged(codec: yy_cobol::Codec, field: &yy_cobol::Field, raw: &[u8], value: &Value) -> bool {
    match codec.decode(field, raw) {
        yy_cobol::Decoded::Empty => value.is_empty() || value.general_text().trim().is_empty(),
        yy_cobol::Decoded::Text(text) => value.general_text().trim_end() == text.trim_end(),
        yy_cobol::Decoded::Num(n) => {
            yy_cobol::Decimal::parse(&value.general_text()).is_some_and(|v| {
                let scale = v.scale.max(n.scale);
                v.rescale(scale) == n.rescale(scale)
            })
        }
        yy_cobol::Decoded::Float(n) => value.as_number() == Some(n),
        yy_cobol::Decoded::Invalid => value.general_text() == yy_cobol::hex_text(raw),
    }
}
pub fn export(book: &crate::workbook::Book, path: &Path) -> Result<()> {
    ensure!(!path.exists(), "Fixed export refuses existing paths");
    let sheet = book.current();
    let spec = sheet
        .fixed
        .as_ref()
        .context("No original fixed-length layout in this sheet")?;
    let raw_col = crate::workbook::raw_column(sheet);
    let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    let mut writer = std::io::BufWriter::new(temporary.reopen()?);
    let header = u64::from(sheet.table.header);
    let count = if sheet.view.rows.is_some() {
        sheet.visible_rows()
    } else {
        sheet
            .record_rows()
            .saturating_sub(header)
            .max(sheet.table.rows)
    };
    let separator = spec.separator.bytes();
    for r in header..header + count {
        let layout = if spec.is_multi() {
            match yy_sheet::fixed::row_layout(&book.doc.ctx, sheet, sheet.source_row(r)) {
                yy_sheet::fixed::RowLayout::Known(layout) => layout.layout.clone(),
                _ => anyhow::bail!("Record {} has no determined layout", r + 1),
            }
        } else {
            spec.layout.clone()
        };
        let original = raw_col
            .map(|c| sheet.get(&book.doc.ctx, r, c))
            .transpose()?;
        let raw = original.as_ref().and_then(|v| match v {
            Value::Text(s) => yy_cobol::parse_hex(s, spec.data_len + separator.len())
                .or_else(|| yy_cobol::parse_hex(s, spec.data_len)),
            _ => None,
        });
        if original.as_ref().is_some_and(|v| !v.is_empty()) {
            ensure!(
                raw.is_some(),
                "Original-record metadata is invalid at row {}",
                r + 1
            );
        }
        let mut bytes = raw.unwrap_or_else(|| vec![spec.codec.charset.space(); spec.data_len]);
        let had_separator = bytes.len() > spec.data_len;
        let new = original.as_ref().is_none_or(Value::is_empty);
        if new {
            for field in &layout.fields {
                let mut issues = yy_cobol::Issues::default();
                spec.codec.encode(
                    field,
                    yy_cobol::Input::Empty,
                    &mut bytes[field.offset..field.offset + field.len],
                    &mut issues,
                );
                ensure!(
                    issues.total() == 0,
                    "Cannot initialize field {}",
                    field.name
                );
            }
        }
        for c in 0..sheet.table.cols() {
            let Some((_, field)) = yy_sheet::fixed::field_at(&book.doc.ctx, sheet, r, c) else {
                continue;
            };
            let value = sheet.get(&book.doc.ctx, r, c)?;
            let target = &mut bytes[field.offset..field.offset + field.len];
            if !new && unchanged(spec.codec, field, target, &value) {
                continue;
            }
            if let Some((_, scale)) = field.kind.digits_scale()
                && !value.is_empty()
                && yy_cobol::parse_hex(&value.general_text(), field.len).is_none()
            {
                let n = yy_cobol::Decimal::parse(&value.general_text())
                    .context("Invalid numeric field")?;
                ensure!(
                    n.truncate(scale).and_then(|v| v.rescale(n.scale)) == Some(n),
                    "Fractional digits would be lost in field {}",
                    field.name
                );
            }
            let mut issues = yy_cobol::Issues::default();
            let input = match &value {
                Value::Empty => yy_cobol::Input::Empty,
                Value::Number(n) => yy_cobol::Input::Number(*n),
                Value::Text(s) => yy_cobol::Input::Text(s),
                Value::Bool(b) => yy_cobol::Input::Bool(*b),
                Value::Error(e) => {
                    anyhow::bail!("Record {}, field {} contains {e}", r + 1, field.name)
                }
            };
            spec.codec.encode(field, input, target, &mut issues);
            ensure!(
                issues.total() == 0,
                "Record {}, field {} cannot be exported losslessly: {issues:?}",
                r + 1,
                field.name
            );
        }
        if !had_separator && (new || r + 1 < header + count) {
            bytes.extend_from_slice(separator);
        }
        writer.write_all(&bytes)?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    temporary.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
