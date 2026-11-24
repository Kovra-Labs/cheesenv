use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::registry::is_plain_file_name;

/// Initial tail size, and cap per poll, so huge logs never flood the UI.
const MAX_CHUNK: u64 = 256 * 1024;

#[derive(Debug, Serialize)]
pub struct LogFile {
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Debug, Serialize)]
pub struct LogChunk {
    pub content: String,
    /// Pass back as `offset` on the next poll to receive only new data.
    pub offset: u64,
    /// True when the file shrank (truncated/rotated); the UI should reset.
    pub reset: bool,
}

pub fn list(dir: &Path) -> Vec<LogFile> {
    let mut out: Vec<LogFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok()?;
            (meta.is_file() && name.ends_with(".log")).then(|| LogFile {
                name,
                size: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Only plain `*.log` names directly inside the logs folder are accessible.
fn resolve(dir: &Path, name: &str) -> AppResult<PathBuf> {
    if !is_plain_file_name(name) || !name.ends_with(".log") {
        return Err(AppError::InvalidInput("log name".into()));
    }
    let path = dir.join(name);
    if !path.is_file() {
        return Err(AppError::NotFound(name.into()));
    }
    Ok(path)
}

pub fn read(dir: &Path, name: &str, offset: Option<u64>) -> AppResult<LogChunk> {
    let path = resolve(dir, name)?;
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    let (mut start, reset) = match offset {
        Some(o) if o <= size => (o, false),
        Some(_) => (0, true),
        None => (0, true),
    };
    if size - start > MAX_CHUNK {
        start = size - MAX_CHUNK;
    }
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((size - start) as usize);
    f.take(size - start).read_to_end(&mut buf)?;
    Ok(LogChunk {
        content: String::from_utf8_lossy(&buf).into_owned(),
        offset: start + buf.len() as u64,
        reset,
    })
}

pub fn clear(dir: &Path, name: &str) -> AppResult<()> {
    let path = resolve(dir, name)?;
    std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_and_incremental_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.log");
        std::fs::write(&p, "one\n").unwrap();
        let c = read(tmp.path(), "a.log", None).unwrap();
        assert_eq!(c.content, "one\n");
        std::fs::write(&p, "one\ntwo\n").unwrap();
        let c2 = read(tmp.path(), "a.log", Some(c.offset)).unwrap();
        assert_eq!(c2.content, "two\n");
        assert!(!c2.reset);
        clear(tmp.path(), "a.log").unwrap();
        let c3 = read(tmp.path(), "a.log", Some(c2.offset)).unwrap();
        assert!(c3.reset);
    }

    #[test]
    fn rejects_paths_outside_logs() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read(tmp.path(), "../secret.log", None).is_err());
        assert!(read(tmp.path(), "settings.json", None).is_err());
        assert!(clear(tmp.path(), "/etc/passwd.log").is_err());
    }
}
