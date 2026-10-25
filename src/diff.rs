//! Shared yymerge bounded-memory engine and paged disk index.
use anyhow::{Context, Result};
use std::{
    fs::File,
    io::BufReader,
    sync::atomic::{AtomicBool, AtomicU64},
};
use yy_buffer::Snapshot;
use yy_merge_core::large::{self, Kind};
pub struct Comparison {
    pub index: large::Index,
    reader: BufReader<File>,
    pub left: Snapshot,
    pub right: Snapshot,
    pub title: String,
    pub top: u64,
}
impl Comparison {
    pub fn new(left: Snapshot, right: Snapshot, title: String) -> Result<Self> {
        let index = large::compare(&left, &right, &AtomicBool::new(false), &AtomicU64::new(0))?
            .context("Comparison cancelled")?;
        let reader = index.open()?;
        Ok(Self {
            index,
            reader,
            left,
            right,
            title,
            top: 0,
        })
    }
    pub fn rows(&mut self, count: usize) -> Result<Vec<large::Row>> {
        Ok(self.index.read_rows(&mut self.reader, self.top, count)?)
    }
    pub fn jump(&mut self, backwards: bool) -> Result<()> {
        let start = if backwards {
            self.top
        } else {
            self.top.saturating_add(1)
        };
        if let Some(row) = self.index.next_change(&mut self.reader, start, backwards)? {
            self.top = row;
        }
        Ok(())
    }
    pub fn text(&self, row: large::Row, left: bool) -> String {
        let (snap, at, len) = if left {
            (&self.left, row.left, row.left_len)
        } else {
            (&self.right, row.right, row.right_len)
        };
        if at == large::NONE {
            return String::new();
        }
        let bytes = snap.read(at..at + len.min(4096));
        let mut text = String::from_utf8_lossy(&bytes)
            .chars()
            .map(|c| match c {
                '\n' => '↵',
                '\r' => '␍',
                '\t' => '→',
                c if c.is_control() => '�',
                c => c,
            })
            .collect::<String>();
        if len > 4096 {
            text.push('…');
        }
        text
    }
}
pub fn changed(kind: Kind) -> bool {
    kind != Kind::Equal
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_navigate_changes_and_preserve_endings() -> Result<()> {
        let mut diff = Comparison::new(
            Snapshot::from_bytes(b"same\na\nb\r\n"),
            Snapshot::from_bytes(b"same\na\ninsert\nb\n"),
            "test".into(),
        )?;
        assert_eq!(diff.index.rows, 4);
        assert_eq!(diff.index.changes, 2);
        diff.jump(false)?;
        assert_eq!(diff.top, 2);
        let rows = diff.rows(1)?;
        assert!(changed(rows[0].kind));
        assert_eq!(diff.text(rows[0], false), "insert↵");
        diff.jump(false)?;
        assert_eq!(diff.top, 3);
        diff.jump(true)?;
        assert_eq!(diff.top, 2);
        Ok(())
    }
}
