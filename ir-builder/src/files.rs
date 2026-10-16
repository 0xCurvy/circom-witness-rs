// SPDX-License-Identifier: GPL-3.0-only
// SPDX-FileCopyrightText: 2026 Curvy Protocol d.o.o.
//! File identity and scratch files, shared by the library and the program.

use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use eyre::{bail, Result, WrapErr};

/// Whether `a` and `b` name the same file: by identity when both exist (which also
/// catches hard links), otherwise by canonical path.
pub fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) {
        use std::os::unix::fs::MetadataExt;
        return a.dev() == b.dev() && a.ino() == b.ino();
    }
    matches!((canonical(a), canonical(b)), (Some(a), Some(b)) if a == b)
}

/// The canonical form of `path`, also for a file that does not exist yet.
fn canonical(path: &Path) -> Option<PathBuf> {
    if let Ok(path) = path.canonicalize() {
        return Some(path);
    }
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.canonicalize().ok()?,
        _ => std::env::current_dir().ok()?,
    };
    Some(parent.join(path.file_name()?))
}

/// A file created exclusively under a fresh name and removed when dropped, unless
/// it is persisted.
pub struct ScratchFile(Option<PathBuf>);

impl ScratchFile {
    /// Creates `<directory>/.<stem>.<unique>.<extension>`.
    pub fn create(directory: &Path, stem: &str, extension: &str) -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        for attempt in 0..100u32 {
            let name = format!(
                ".{stem}.{}-{nanos}-{attempt}.{extension}",
                std::process::id()
            );
            let path = directory.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self(Some(path))),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).wrap_err_with(|| {
                        format!("could not create a file in {}", directory.display())
                    })
                }
            }
        }
        bail!("could not find a free file name in {}", directory.display())
    }

    pub fn path(&self) -> &Path {
        self.0
            .as_deref()
            .expect("a scratch file has a path until persisted")
    }

    /// Moves the file to `destination`, replacing what is there, and keeps it.
    pub fn persist(mut self, destination: &Path) -> Result<()> {
        let path = self
            .0
            .take()
            .expect("a scratch file has a path until persisted");
        fs::rename(&path, destination).wrap_err_with(|| {
            let _ = fs::remove_file(&path);
            format!("could not move the result to {}", destination.display())
        })
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}
