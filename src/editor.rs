use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use yy_core::{Document, OpenOptions, Selection, SelectionSet, Version};

pub struct Editor {
    pub doc: Document,
    pub terminal: Option<crate::shell::Shell>,
    pub term_back: usize,
    pub raw: bool,
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
            terminal: None,
            term_back: 0,
            saved: doc.version(),
            doc,
            raw: false,
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
            terminal: None,
            term_back: 0,
            saved: doc.version(),
            raw,
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
                    if snapshot.byte_at(prev) == Some(b'\n') {
                        p = prev;
                    }
                }
                while p > 0 && cells < width.max(1) {
                    let prev = yy_core::motion::prev_grapheme(snapshot, p);
                    if snapshot.byte_at(prev) == Some(b'\n') {
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
        let snap = self.doc.snapshot();
        let mut at = self.top.min(snap.len());
        let mut rows = Vec::new();
        for _ in 0..count {
            let start = at;
            let mut text = String::new();
            let mut columns = 0;
            while at < snap.len() && at - start < 16384 {
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
}
