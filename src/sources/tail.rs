//! Follow a growing line-oriented file (a transcript) without re-reading it.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use crate::{Error, Result};

/// A reader positioned at the end of what has been consumed so far.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl Tail {
    /// Open `path` and return the complete lines in its last `max_backfill`
    /// bytes (to rebuild current state), positioned to read only new data
    /// afterwards. A first line cut by the backfill window is dropped.
    pub fn open_at_end(path: &Path, max_backfill: u64) -> Result<(Tail, Vec<String>)> {
        let len = file_len(path)?;
        let start = len.saturating_sub(max_backfill);
        let mut tail = Tail {
            path: path.to_path_buf(),
            offset: start,
            partial: String::new(),
        };
        let mut lines = tail.read_new()?;
        if start > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        Ok((tail, lines))
    }

    /// Complete lines appended since the last call. An unterminated last line
    /// is kept until its newline arrives. If the file shrank, start over.
    pub fn read_new(&mut self) -> Result<Vec<String>> {
        let len = file_len(&self.path)?;
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.path).map_err(|source| self.io(source))?;
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|source| self.io(source))?;
        let mut bytes = Vec::new();
        file.take(len - self.offset)
            .read_to_end(&mut bytes)
            .map_err(|source| self.io(source))?;
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));

        let mut lines = Vec::new();
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            lines.push(line.trim_end_matches(['\n', '\r']).to_string());
        }
        Ok(lines)
    }

    /// The file being followed.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn io(&self, source: std::io::Error) -> Error {
        Error::Io {
            path: self.path.clone(),
            source,
        }
    }
}

fn file_len(path: &Path) -> Result<u64> {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn append(path: &std::path::Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open fixture for append");
        file.write_all(text.as_bytes()).expect("append");
    }

    #[test]
    fn tail_returns_only_complete_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        append(&path, "");
        let (mut tail, backfill) = Tail::open_at_end(&path, 1024).expect("open");
        assert!(backfill.is_empty());
        append(&path, "a\nb");
        assert_eq!(tail.read_new().expect("read"), ["a"]);
        append(&path, "c\n");
        assert_eq!(tail.read_new().expect("read"), ["bc"]);
        assert!(tail.read_new().expect("read").is_empty());
    }

    #[test]
    fn tail_open_at_end_skips_old_history() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("big.jsonl");
        let line = format!("{}\n", "x".repeat(99)); // 100 bytes per line
        append(&path, &line.repeat(10_486)); // ~1 MiB
        append(&path, "last\n");
        let (_tail, backfill) = Tail::open_at_end(&path, 256 * 1024).expect("open");
        assert!(backfill.len() < 2_700, "read {} lines", backfill.len());
        assert!(backfill.len() > 2_500, "read {} lines", backfill.len());
        assert_eq!(backfill.last().map(String::as_str), Some("last"));
        assert!(backfill[..backfill.len() - 1].iter().all(|l| l.len() == 99));
    }

    #[test]
    fn tail_open_at_end_keeps_small_file_whole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("small.jsonl");
        append(&path, "one\ntwo\nthr");
        let (mut tail, backfill) = Tail::open_at_end(&path, 1024).expect("open");
        assert_eq!(backfill, ["one", "two"]);
        append(&path, "ee\n");
        assert_eq!(tail.read_new().expect("read"), ["three"]);
    }

    #[test]
    fn tail_resets_when_file_shrinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.jsonl");
        append(&path, "old line one\nold line two\n");
        let (mut tail, _) = Tail::open_at_end(&path, 1024).expect("open");
        std::fs::write(&path, "new\n").expect("truncate");
        assert_eq!(tail.read_new().expect("read"), ["new"]);
    }

    #[test]
    fn missing_file_is_an_io_error_naming_the_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nope.jsonl");
        let err = Tail::open_at_end(&path, 1024).expect_err("missing");
        assert!(err.to_string().contains("nope.jsonl"));
    }
}
