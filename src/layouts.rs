use anyhow::{Context, Result, ensure};
use std::{path::Path, sync::Arc};
use yy_sheet::{
    catalog::LayoutDef,
    fixed::{FixedSpec, RecordSep},
};

pub fn open_data(path: &Path, spec: FixedSpec) -> Result<crate::editor::Editor> {
    let mut editor = crate::editor::Editor::open_raw(path)?;
    if spec.is_multi() {
        let book = crate::workbook::Book::import(
            editor.doc.snapshot(),
            Some(&crate::data::Mode::Fixed(Arc::new(spec))),
        )?;
        editor.spreadsheet = true;
        editor.path = None;
        editor.digest = None;
        editor.book = Some(book);
    } else {
        editor.table = Some(crate::data::Grid::new(crate::data::Mode::Fixed(Arc::new(
            spec,
        ))));
    }
    Ok(editor)
}
pub fn load(path: &Path) -> Result<FixedSpec> {
    let def = yy_sheet::catalog::read_path(path).map_err(anyhow::Error::msg)?;
    def.to_spec(
        yy_cobol::Codec {
            charset: yy_cobol::Charset::Ms932,
            little_endian: false,
        },
        RecordSep::None,
    )
    .map_err(anyhow::Error::msg)
}
pub fn save(path: &Path, spec: &FixedSpec) -> Result<()> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yyl")),
        "Use a .yyl extension"
    );
    let mut def = LayoutDef::from_spec(spec, "");
    if !spec.is_multi() && spec.data_len != spec.layout.record_len {
        def.data_len = Some(spec.data_len);
        def.layouts[0].0 = "Layout1".into();
    }
    let temp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(temp.path(), yy_sheet::catalog::def_text(&def))?;
    temp.persist_noclobber(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn infer(snapshot: &yy_buffer::Snapshot, input: &str) -> Result<String> {
    let parts = input.split(';').map(str::trim).collect::<Vec<_>>();
    ensure!(parts.len() == 3, "Use LENGTH or auto;CHARSET;SEPARATOR");
    let charset = yy_cobol::Charset::from_name(parts[1]).context("Unknown fixed character set")?;
    let separator = RecordSep::from_name(parts[2]).context("Separator: none/crlf/lf/nl")?;
    let sample = snapshot.read(0..snapshot.len().min(1024 * 1024));
    let guesses = yy_sheet::fixed::infer_record_lengths(&sample, snapshot.len(), charset);
    let (length, separator) = if parts[0].eq_ignore_ascii_case("auto") {
        let guess = guesses
            .first()
            .context("No record-length candidate; specify a byte length")?;
        (guess.data_len, guess.separator)
    } else {
        (
            parts[0].parse::<usize>().context("Invalid record length")?,
            separator,
        )
    };
    ensure!(
        length > 0 && length <= 1024 * 1024,
        "Record length: 1..1048576 bytes"
    );
    let codec = yy_cobol::Codec {
        charset,
        little_endian: false,
    };
    let layouts = yy_sheet::fixed::infer_layouts(&sample, length, codec, separator, true)
        .map_err(anyhow::Error::msg)?;
    let def = LayoutDef {
        layouts,
        data_len: Some(length),
        charset: Some(charset),
        separator: Some(separator),
        little_endian: Some(false),
        description: format!(
            "INFERRED: review/edit before use. Candidates: {}",
            guesses
                .iter()
                .take(5)
                .map(|g| format!(
                    "{} bytes / {} / score {}",
                    g.data_len,
                    g.separator.name(),
                    g.score
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    Ok(yy_sheet::catalog::def_text(&def))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padded_layout_roundtrip_and_editable_inference() -> Result<()> {
        let mut spec = FixedSpec::new(
            "01 REC.\n 05 ID PIC X(4).\n",
            yy_cobol::Codec {
                charset: yy_cobol::Charset::Ms932,
                little_endian: false,
            },
            RecordSep::None,
        )
        .map_err(anyhow::Error::msg)?;
        spec.data_len = 9;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("layout.yyl");
        save(&path, &spec)?;
        let read = load(&path)?;
        assert_eq!(read.data_len, 9);
        assert_eq!(read.layout.fields[0].name, "ID");
        assert!(save(&path, &spec).is_err());
        let text = infer(
            &yy_buffer::Snapshot::from_bytes(b"ABCD00001EFGH00002"),
            "9;MS932;none",
        )?;
        assert!(text.contains("INFERRED"));
        let inferred = dir.path().join("inferred.yyl");
        std::fs::write(&inferred, text)?;
        assert_eq!(load(&inferred)?.data_len, 9);
        Ok(())
    }
}
