use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

const READ_LIMIT: u64 = 1024 * 1024;
const LINE_LIMIT: usize = 64 * 1024;
const CHECKPOINT: u64 = 128;

#[derive(Debug, Default)]
pub struct Batch {
    pub reset: bool,
    pub lines: Vec<String>,
}

/// Reopens read-only on each poll so replacement/rotation and Windows sharing work.
/// Memory and per-poll IO are bounded; incomplete UTF-8 lines wait for their newline.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    offset: u64,
    created: Option<SystemTime>,
    checkpoint: Vec<u8>,
    pending: Vec<u8>,
    dropping_line: bool,
    seen: bool,
}

impl Tail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            created: None,
            checkpoint: Vec::new(),
            pending: Vec::new(),
            dropping_line: false,
            seen: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn poll(&mut self) -> io::Result<Batch> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // A disappearance followed by recreation resets even if timestamps/prefix match.
                if self.seen {
                    self.created = None;
                    self.checkpoint.clear();
                    self.offset = 0;
                    self.pending.clear();
                    self.dropping_line = false;
                    self.seen = false;
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        let created = metadata.created().ok();
        let mut reset = !self.seen || metadata.len() < self.offset || self.created != created;
        if !reset && !self.checkpoint.is_empty() {
            let start = self.offset.saturating_sub(self.checkpoint.len() as u64);
            file.seek(SeekFrom::Start(start))?;
            let mut checkpoint = vec![0; self.checkpoint.len()];
            reset = file.read_exact(&mut checkpoint).is_err() || checkpoint != self.checkpoint;
        }
        if reset {
            self.offset = 0;
            self.pending.clear();
            self.dropping_line = false;
        }
        self.created = created;
        self.seen = true;
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        (&mut file).take(READ_LIMIT).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        let mut expected = if reset {
            Vec::new()
        } else {
            self.checkpoint.clone()
        };
        expected.extend_from_slice(&bytes);
        if expected.len() > CHECKPOINT as usize {
            expected = expected[expected.len() - CHECKPOINT as usize..].to_vec();
        }
        let mut batch = Batch {
            reset,
            lines: Vec::new(),
        };
        for byte in bytes {
            if byte == b'\n' {
                if !self.dropping_line {
                    batch.lines.push(
                        String::from_utf8_lossy(&self.pending)
                            .trim_end_matches('\r')
                            .to_owned(),
                    );
                }
                self.pending.clear();
                self.dropping_line = false;
            } else if !self.dropping_line {
                if self.pending.len() < LINE_LIMIT {
                    self.pending.push(byte);
                } else {
                    self.pending.clear();
                    self.dropping_line = true;
                }
            }
        }
        let checkpoint_len = self.offset.min(CHECKPOINT) as usize;
        file.seek(SeekFrom::Start(self.offset - checkpoint_len as u64))?;
        let mut checkpoint = vec![0; checkpoint_len];
        // If the game rewrites between reading and checkpointing, retry from scratch.
        if file.read_exact(&mut checkpoint).is_err() || checkpoint != expected {
            self.seen = false;
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Log changed while reading",
            ));
        }
        self.checkpoint = checkpoint;
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn partial_lines_and_unicode_survive_multiple_polls() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut tail = Tail::new(file.path());
        std::fs::write(file.path(), b"first\r\nsecond \xc3").unwrap();
        let batch = tail.poll().unwrap();
        assert!(batch.reset);
        assert_eq!(batch.lines, ["first"]);
        std::fs::OpenOptions::new()
            .append(true)
            .open(file.path())
            .unwrap()
            .write_all(b"\xa9\n")
            .unwrap();
        let batch = tail.poll().unwrap();
        assert!(!batch.reset);
        assert_eq!(batch.lines, ["second é"]);
        assert!(tail.poll().unwrap().lines.is_empty());
    }
    #[test]
    fn truncation_and_same_size_rewrite_are_detected() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "old data\n").unwrap();
        let mut tail = Tail::new(file.path());
        tail.poll().unwrap();
        std::fs::write(file.path(), "new data\n").unwrap();
        let batch = tail.poll().unwrap();
        assert!(batch.reset);
        assert_eq!(batch.lines, ["new data"]);
        std::fs::write(file.path(), "x\n").unwrap();
        let batch = tail.poll().unwrap();
        assert!(batch.reset);
        assert_eq!(batch.lines, ["x"]);
    }
    #[test]
    fn replacement_missing_file_and_oversized_lines_recover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Game.log");
        let mut tail = Tail::new(&path);
        assert!(tail.poll().is_err());
        std::fs::write(&path, "old\n").unwrap();
        tail.poll().unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(tail.poll().is_err());
        std::fs::write(&path, format!("{}\nok\n", "a".repeat(LINE_LIMIT + 1))).unwrap();
        let batch = tail.poll().unwrap();
        assert!(batch.reset);
        assert_eq!(batch.lines, ["ok"]);
    }
}
