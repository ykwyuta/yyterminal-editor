use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use yy_core::{Document, OpenOptions, Selection, SelectionSet, Version};

#[derive(Default)]
struct RowCache {
    snapshot: Option<yy_buffer::Snapshot>,
    width: usize,
    rows: Vec<(u64, u64, String)>,
}
pub struct Editor {
    pub doc: Document,
    row_cache: std::cell::RefCell<RowCache>,
    pub syntax: yyterminal_editor::syntax::EditorHighlighter,
    pub syntax_language: Option<String>,
    pub terminal: Option<crate::shell::Shell>,
    pub term_back: usize,
    pub raw: bool,
    pub database: bool,
    pub er_edit: Option<(u64, usize)>,
    pub spreadsheet: bool,
    pub book: Option<crate::workbook::Book>,
    pub table: Option<crate::data::Grid>,
    pub path: Option<PathBuf>,
    pub top: u64,
    pub saved: Version,
    pub digest: Option<[u8; 32]>,
    // Document must be dropped before its private backing directory.
    _storage: Option<TempDir>,
}

pub fn digest(path: &Path) -> Result<[u8; 32]> {
    let mut f = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().into())
}

impl Editor {
    pub fn empty() -> Self {
        let doc = Document::new_empty();
        Self {
            row_cache: Default::default(),
            syntax: Default::default(),
            syntax_language: None,
            terminal: None,
            term_back: 0,
            saved: doc.version(),
            doc,
            raw: false,
            database: false,
            er_edit: None,
            spreadsheet: false,
            book: None,
            table: None,
            path: None,
            top: 0,
            digest: None,
            _storage: None,
        }
    }
    pub fn open(path: &Path, encoding: Option<yy_core::Encoding>) -> Result<Self> {
        Self::load(path, encoding, false)
    }
    pub fn open_raw(path: &Path) -> Result<Self> {
        Self::load(path, None, true)
    }
    fn load(path: &Path, encoding: Option<yy_core::Encoding>, raw: bool) -> Result<Self> {
        let path = path.canonicalize().context("Cannot open file")?;
        let storage = tempfile::tempdir()?;
        let copy = storage.path().join("document");
        let mut input = File::open(&path)?;
        let mut output = File::create(&copy)?;
        let mut hash = Sha256::new();
        let mut buf = vec![0; 1024 * 1024];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n])?;
            hash.update(&buf[..n]);
        }
        drop(output);
        let copied: [u8; 32] = hash.finalize().into();
        if digest(&path)? != copied {
            bail!("File changed while opening; retry");
        }
        let mut doc = Document::open_with(
            &copy,
            &OpenOptions {
                encoding,
                raw,
                ..OpenOptions::default()
            },
        )?;
        // Transcoding runs on this opening worker, never on the UI thread.
        if doc.is_loading() {
            let pool = yy_jobs::JobPool::new(2);
            doc.start_indexing(&pool, std::sync::Arc::new(|| {}));
            doc.wait_loading();
        }
        if let Some(error) = doc.load_error() {
            bail!("{error}");
        }
        Ok(Self {
            row_cache: Default::default(),
            syntax: Default::default(),
            syntax_language: None,
            terminal: None,
            term_back: 0,
            saved: doc.version(),
            raw,
            database: false,
            er_edit: None,
            spreadsheet: false,
            book: None,
            table: None,
            doc,
            path: Some(path),
            top: 0,
            digest: Some(copied),
            _storage: Some(storage),
        })
    }
    pub fn modified(&self) -> bool {
        self.book.as_ref().is_some_and(|b| b.doc.dirty) || self.doc.version() != self.saved
    }
    pub fn caret(&self) -> u64 {
        self.doc.selections().primary().head
    }
    pub fn move_to(&mut self, offset: u64, extend: bool) {
        let anchor = if extend {
            self.doc.selections().primary().anchor
        } else {
            offset
        };
        self.doc
            .set_selections(SelectionSet::single(Selection::new(anchor, offset)));
    }
    pub fn offset_at(&self, start: u64, end: u64, column: usize) -> u64 {
        let snapshot = self.doc.snapshot();
        let mut at = start;
        let mut cells = 0;
        while at < end {
            let next = yy_core::motion::next_grapheme(snapshot, at);
            let bytes = snapshot.read(at..next);
            if bytes == b"\n" || bytes == b"\r" {
                break;
            }
            let text = String::from_utf8_lossy(&bytes);
            let width = if text == "\t" {
                4
            } else {
                unicode_width::UnicodeWidthStr::width(text.as_ref())
            };
            if cells + width > column {
                break;
            }
            cells += width;
            at = next;
        }
        at
    }
    pub fn scroll_rows(&mut self, down: bool, count: usize, width: usize) {
        if down {
            let rows = self.rows(count + 1, width);
            if let Some(row) = rows.get(count).or_else(|| rows.last()) {
                self.top = row.0;
            }
        } else {
            for _ in 0..count {
                let snapshot = self.doc.snapshot();
                let mut p = self.top;
                let mut cells = 0;
                if p > 0 {
                    let prev = yy_core::motion::prev_grapheme(snapshot, p);
                    if snapshot.byte_at(p - 1) == Some(b'\n') {
                        p = prev;
                    }
                }
                while p > 0 && cells < width.max(1) {
                    let prev = yy_core::motion::prev_grapheme(snapshot, p);
                    if snapshot.byte_at(p - 1) == Some(b'\n') {
                        break;
                    }
                    let bytes = snapshot.read(prev..p);
                    let text = String::from_utf8_lossy(&bytes);
                    cells += if text == "\t" {
                        4
                    } else {
                        unicode_width::UnicodeWidthStr::width(text.as_ref())
                    };
                    p = prev;
                }
                self.top = p;
            }
        }
    }
    /// Bounded visual rows; even a single multi-gigabyte line never gets materialized.
    pub fn rows(&self, count: usize, width: usize) -> Vec<(u64, u64, String)> {
        if count == 0 {
            return vec![];
        }
        if count > 256 {
            return self.layout_rows(self.top, count, width);
        }
        let snapshot = self.doc.snapshot();
        let top = self.top.min(snapshot.len());
        let mut cache = self.row_cache.borrow_mut();
        if cache.width != width || !cache.snapshot.as_ref().is_some_and(|s| s.ptr_eq(snapshot)) {
            cache.snapshot = Some(snapshot.clone());
            cache.width = width;
            cache.rows.clear();
        }
        if let Some(index) = cache.rows.iter().position(|r| r.0 == top) {
            if index + count <= cache.rows.len()
                || cache.rows.last().is_some_and(|r| r.1 == snapshot.len())
            {
                return cache.rows[index..(index + count).min(cache.rows.len())].to_vec();
            }
            cache.rows.drain(..index);
            let start = cache.rows.last().unwrap().1;
            let extra = (count + 32).min(256).saturating_sub(cache.rows.len());
            cache.rows.extend(self.layout_rows(start, extra, width));
        } else {
            cache.rows = self.layout_rows(top, (count + 32).min(256), width);
        }
        cache.rows.iter().take(count).cloned().collect()
    }
    fn layout_rows(&self, top: u64, count: usize, width: usize) -> Vec<(u64, u64, String)> {
        let snap = self.doc.snapshot();
        let mut at = top.min(snap.len());
        let mut rows = Vec::new();
        let mut window_start = at;
        let mut window = snap.read(at..at.saturating_add(32768).min(snap.len()));
        for _ in 0..count {
            let start = at;
            let mut text = String::new();
            let mut columns = 0;
            while at < snap.len() && at - start < 16384 {
                if at + 1 >= window_start + window.len() as u64 {
                    window_start = at;
                    window = snap.read(at..at.saturating_add(32768).min(snap.len()));
                }
                let index = (at - window_start) as usize;
                let byte = window[index];
                if byte.is_ascii() && window.get(index + 1).is_none_or(u8::is_ascii) {
                    if byte == b'\n' {
                        at += 1;
                        break;
                    }
                    if byte == b'\r' {
                        at += 1;
                        if window.get(index + 1) == Some(&b'\n') {
                            at += 1;
                            break;
                        }
                        continue;
                    }
                    let w = if byte == b'\t' { 4 } else { 1 };
                    if columns + w > width.max(1) && at > start {
                        break;
                    }
                    if byte == b'\t' {
                        text.push_str("    ");
                    } else {
                        text.push(if byte.is_ascii_control() {
                            '�'
                        } else {
                            char::from(byte)
                        });
                    }
                    columns += w;
                    at += 1;
                    continue;
                }
                let end = yy_core::motion::next_grapheme(snap, at);
                let bytes = snap.read(at..end);
                if bytes == b"\n" {
                    at = end;
                    break;
                }
                if bytes == b"\r" {
                    at = end;
                    continue;
                }
                let s = String::from_utf8_lossy(&bytes);
                let s = if s == "\t" { "    ".into() } else { s };
                let w = unicode_width::UnicodeWidthStr::width(s.as_ref());
                if columns + w > width.max(1) && at > start {
                    break;
                }
                for ch in s.chars() {
                    text.push(if ch.is_control() { '�' } else { ch });
                }
                columns += w;
                at = end;
            }
            rows.push((start, at, text));
            if at == snap.len() {
                break;
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scrolling_up_preserves_blank_crlf_rows() {
        let mut e = Editor::empty();
        e.doc.convert_eol(yy_core::Eol::CrLf).unwrap();
        e.doc.insert_text("first\r\n\r\nlast", false);
        e.top = "first\r\n\r\n".len() as u64;
        e.scroll_rows(false, 1, 80);
        assert_eq!(e.top, "first\r\n".len() as u64);
        assert_eq!(e.rows(1, 80)[0].2, "");
        e.scroll_rows(false, 1, 80);
        assert_eq!(e.top, 0);
    }
    #[test]
    fn buffered_rows_keep_crlf_combining_emoji_and_tab_boundaries() {
        let mut e = Editor::empty();
        e.doc.convert_eol(yy_core::Eol::CrLf).unwrap();
        e.doc.insert_text("abc\r\n日本語\ne\u{301} 👩‍💻\tX\n", false);
        let rows = e.rows(10, 6);
        assert_eq!(
            rows.iter().map(|r| r.2.as_str()).collect::<Vec<_>>(),
            vec!["abc", "日本語", "e\u{301} 👩‍💻", "    X"]
        );
        for pair in rows.windows(2) {
            assert_eq!(pair[0].1, pair[1].0);
        }
        let mut source = "x".repeat(32767);
        source.push_str("\r\ne\u{301}👩‍💻END");
        e.doc.select_all();
        e.doc.insert_text(&source, false);
        assert!(
            e.rows(8, 16384)
                .iter()
                .any(|r| r.2.contains("e\u{301}👩‍💻END"))
        );
    }
    #[test]
    #[ignore = "manual scrolling performance measurement"]
    fn scrolling_row_layout_benchmark() {
        let mut e = Editor::empty();
        e.doc.insert_text(
            &"let value = \"日本語 👩‍💻\";\t// abcdefghijklmnopqrstuvwxyz\n".repeat(5000),
            false,
        );
        let started = std::time::Instant::now();
        for _ in 0..200 {
            std::hint::black_box(e.rows(40, 120));
            e.scroll_rows(true, 3, 120);
        }
        eprintln!("SCROLL_LAYOUT 200 frames: {:?}", started.elapsed());
    }
    #[test]
    fn private_snapshot_survives_external_truncation_and_keeps_bytes() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("日本語.txt");
        std::fs::write(&p, b"a\r\nb\xff")?;
        let e = Editor::open(&p, None)?;
        std::fs::write(&p, b"")?;
        assert!(!e.rows(10, 80).is_empty());
        let out = dir.path().join("out");
        yy_io::save_snapshot(
            e.doc.snapshot(),
            &out,
            &e.doc.save_format(e.doc.encoding(), e.doc.has_bom()),
            None,
        )?;
        assert_eq!(std::fs::read(out)?, b"a\r\nb\xff");
        Ok(())
    }
    #[test]
    fn edits_and_undo_share_yy_model() {
        let mut e = Editor::empty();
        e.doc.insert_text("日本語", false);
        assert!(e.modified());
        e.doc.undo();
        assert!(!e.modified());
    }
    #[test]
    fn explicit_host_encoding_round_trip() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("host.dat");
        std::fs::write(&path, [0xc1, 0xc2, 0xc3, 0x15])?;
        let encoding = yy_core::Encoding::from_name("IBM-037").context("Missing IBM-037")?;
        let e = Editor::open(&path, Some(encoding))?;
        assert!(e.rows(3, 80)[0].2.contains("ABC"));
        let out = dir.path().join("out");
        yy_io::save_snapshot(
            e.doc.snapshot(),
            &out,
            &e.doc.save_format(encoding, false),
            None,
        )?;
        assert_eq!(std::fs::read(out)?, [0xc1, 0xc2, 0xc3, 0x15]);
        Ok(())
    }
    #[test]
    fn large_file_uses_disk_backing_and_bounded_rows() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("large");
        File::create(&path)?.set_len(64 * 1024 * 1024)?;
        let e = Editor::open(&path, Some(yy_core::Encoding::Utf8))?;
        assert_eq!(e.doc.snapshot().len(), 64 * 1024 * 1024);
        assert!(e.rows(24, 80).iter().all(|r| r.1 - r.0 <= 80));
        Ok(())
    }
    #[test]
    fn long_line_render_is_bounded() {
        let mut e = Editor::empty();
        e.doc.insert_text(&"a".repeat(1_000_000), false);
        let rows = e.rows(2, 80);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].1, 160);
    }
    #[test]
    fn cached_rows_match_fresh_layout_after_scroll_resize_edit_and_undo() {
        let mut e = Editor::empty();
        e.doc
            .insert_text(&"日本語 e\u{301} 👩‍💻\ttext\r\n\r\n".repeat(200), false);
        for width in [80, 7, 1, 120] {
            for _ in 0..8 {
                assert_eq!(e.rows(12, width), e.layout_rows(e.top, 12, width));
                e.scroll_rows(true, 3, width);
            }
            e.doc.insert_text("changed", false);
            assert_eq!(e.rows(12, width), e.layout_rows(e.top, 12, width));
            e.doc.undo();
            assert_eq!(e.rows(12, width), e.layout_rows(e.top, 12, width));
        }
        e.doc = yy_core::Document::from_text("replacement");
        e.top = 0;
        assert_eq!(e.rows(12, 80), e.layout_rows(0, 12, 80));
        assert!(e.rows(0, 80).is_empty());
    }
}
