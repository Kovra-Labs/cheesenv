use std::fs::File;
use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Extracts a verified archive into `dest` (an empty directory), rejecting
/// absolute paths, `..`, links escaping `dest`, and writes through symlinks.
pub fn extract(archive: &Path, format: &str, dest: &Path, strip: usize, bin_name: &str) -> AppResult<()> {
    let root = dest.canonicalize()?;
    match format {
        "tar.gz" => extract_tar(flate2::read::GzDecoder::new(File::open(archive)?), &root, strip),
        "tar.xz" => extract_tar(liblzma::read::XzDecoder::new(File::open(archive)?), &root, strip),
        "zip" => extract_zip(archive, &root, strip),
        "binary" => {
            let out = root.join(bin_name);
            std::fs::copy(archive, &out)?;
            set_mode(&out, 0o755)?;
            Ok(())
        }
        other => Err(AppError::InvalidInput(format!("archive format {other}"))),
    }
}

fn unsafe_entry(p: &Path) -> AppError {
    AppError::UnsafeArchive(p.display().to_string())
}

/// Normalises an entry path, dropping `strip` leading components.
/// Returns `None` for entries that vanish after stripping.
fn sanitize(path: &Path, strip: usize) -> AppResult<Option<PathBuf>> {
    let mut parts = Vec::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => parts.push(p),
            Component::CurDir => {}
            _ => return Err(unsafe_entry(path)),
        }
    }
    if parts.len() <= strip {
        return Ok(None);
    }
    Ok(Some(parts[strip..].iter().collect()))
}

/// The parent directory of `out` must resolve (following symlinks) inside root.
fn checked_parent(root: &Path, out: &Path) -> AppResult<()> {
    let parent = out.parent().unwrap_or(root);
    std::fs::create_dir_all(parent)?;
    if parent.canonicalize()?.starts_with(root) {
        Ok(())
    } else {
        Err(unsafe_entry(out))
    }
}

/// Lexically resolves `target` relative to `base` (both relative to root) and
/// checks it never climbs above the root.
fn link_stays_inside(base: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return false;
    }
    let mut depth: Vec<&std::ffi::OsStr> = base
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p),
            _ => None,
        })
        .collect();
    for c in target.components() {
        match c {
            Component::Normal(p) => depth.push(p),
            Component::CurDir => {}
            Component::ParentDir => {
                if depth.pop().is_none() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn extract_tar(stream: impl std::io::Read, root: &Path, strip: usize) -> AppResult<()> {
    let mut ar = tar::Archive::new(stream);
    ar.set_preserve_permissions(true);
    ar.set_overwrite(true);
    for entry in ar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(rel) = sanitize(&path, strip)? else { continue };
        let kind = entry.header().entry_type();
        let out = root.join(&rel);

        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry.link_name()?.ok_or_else(|| unsafe_entry(&path))?.into_owned();
            let ok = if kind.is_symlink() {
                link_stays_inside(rel.parent().unwrap_or(Path::new("")), &target)
            } else {
                // Hard-link targets are archive paths; they get stripped too.
                matches!(sanitize(&target, strip), Ok(Some(_)))
            };
            if !ok {
                return Err(unsafe_entry(&path));
            }
            if kind.is_hard_link() {
                let src = root.join(sanitize(&target, strip)?.unwrap_or_default());
                checked_parent(root, &out)?;
                checked_parent(root, &src)?;
                std::fs::hard_link(&src, &out)?;
                continue;
            }
        } else if !(kind.is_file() || kind.is_dir()) {
            continue; // devices, fifos, etc. are never needed
        }

        checked_parent(root, &out)?;
        entry.unpack(&out)?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, root: &Path, strip: usize) -> AppResult<()> {
    let zerr = |e: zip::result::ZipError| AppError::UnsafeArchive(e.to_string());
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(zerr)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(zerr)?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| AppError::UnsafeArchive(format!("zip entry #{i}")))?;
        let Some(rel) = sanitize(&name, strip)? else { continue };
        let mode = file.unix_mode();
        if mode.is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(unsafe_entry(&name)); // symlinks are not expected in zips
        }
        let out = root.join(&rel);
        checked_parent(root, &out)?;
        if file.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        let mut w = File::create(&out)?;
        std::io::copy(&mut file, &mut w)?;
        if let Some(m) = mode {
            set_mode(&out, m & 0o777)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rules() {
        assert_eq!(
            sanitize(Path::new("php-8/bin/php"), 1).unwrap(),
            Some(PathBuf::from("bin/php"))
        );
        assert_eq!(sanitize(Path::new("php-8/"), 1).unwrap(), None);
        assert!(sanitize(Path::new("../evil"), 0).is_err());
        assert!(sanitize(Path::new("/etc/passwd"), 0).is_err());
        assert!(sanitize(Path::new("a/../../b"), 0).is_err());
    }

    #[test]
    fn link_rules() {
        assert!(link_stays_inside(Path::new("bin"), Path::new("../lib/x")));
        assert!(!link_stays_inside(Path::new("bin"), Path::new("../../x")));
        assert!(!link_stays_inside(Path::new(""), Path::new("/etc/passwd")));
    }

    fn tar_with(build: impl FnOnce(&mut tar::Builder<flate2::write::GzEncoder<File>>)) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let f = File::create(dir.path().join("a.tar.gz")).unwrap();
        let mut b = tar::Builder::new(flate2::write::GzEncoder::new(f, flate2::Compression::fast()));
        build(&mut b);
        b.into_inner().unwrap().finish().unwrap();
        std::fs::create_dir(dir.path().join("out")).unwrap();
        dir
    }

    fn file_header(path: &str, len: u64) -> tar::Header {
        let mut h = tar::Header::new_gnu();
        // Bypass Builder's own path checks to simulate a hostile archive.
        h.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
        h.set_size(len);
        h.set_mode(0o644);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        h
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> tempfile::TempDir {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut w = zip::ZipWriter::new(File::create(dir.path().join("a.zip")).unwrap());
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        std::fs::create_dir(dir.path().join("out")).unwrap();
        dir
    }

    #[test]
    fn zip_extracts_and_rejects_traversal() {
        let ok = zip_with(&[("nginx-1/nginx.exe", b"MZ")]);
        extract(&ok.path().join("a.zip"), "zip", &ok.path().join("out"), 1, "").unwrap();
        assert_eq!(std::fs::read(ok.path().join("out/nginx.exe")).unwrap(), b"MZ");

        let bad = zip_with(&[("../../evil.txt", b"x")]);
        let r = extract(&bad.path().join("a.zip"), "zip", &bad.path().join("out"), 0, "");
        assert!(matches!(r, Err(AppError::UnsafeArchive(_))));
    }

    #[test]
    fn tar_xz_extracts() {
        let dir = tempfile::tempdir().unwrap();
        let f = File::create(dir.path().join("a.tar.xz")).unwrap();
        let mut b = tar::Builder::new(liblzma::write::XzEncoder::new(f, 1));
        b.append(&file_header("mysql-8/bin/mysqld", 2), &b"ok"[..]).unwrap();
        b.into_inner().unwrap().finish().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        extract(&dir.path().join("a.tar.xz"), "tar.xz", &out, 1, "").unwrap();
        assert_eq!(std::fs::read(out.join("bin/mysqld")).unwrap(), b"ok");
    }

    #[test]
    fn tar_extracts_and_strips() {
        let dir = tar_with(|b| {
            let h = file_header("top/bin/tool", 2);
            b.append(&h, &b"hi"[..]).unwrap();
        });
        let out = dir.path().join("out");
        extract(&dir.path().join("a.tar.gz"), "tar.gz", &out, 1, "").unwrap();
        assert_eq!(std::fs::read(out.join("bin/tool")).unwrap(), b"hi");
    }

    #[test]
    fn tar_rejects_traversal() {
        let dir = tar_with(|b| {
            let h = file_header("../../escape", 1);
            b.append(&h, &b"x"[..]).unwrap();
        });
        let out = dir.path().join("out");
        let r = extract(&dir.path().join("a.tar.gz"), "tar.gz", &out, 0, "");
        assert!(matches!(r, Err(AppError::UnsafeArchive(_))));
        assert!(!dir.path().join("escape").exists());
    }

    #[test]
    fn tar_rejects_escaping_symlink_and_write_through() {
        let dir = tar_with(|b| {
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            b.append_link(&mut h, "link", "../../outside").unwrap();
        });
        let out = dir.path().join("out");
        let r = extract(&dir.path().join("a.tar.gz"), "tar.gz", &out, 0, "");
        assert!(matches!(r, Err(AppError::UnsafeArchive(_))));
    }
}
