//! Every filesystem read and write the crate performs.
//!
//! Discovery constantly asks questions about paths that may not exist, so these
//! helpers answer with `Option` and `bool` rather than making each caller match
//! on [`ErrorKind::NotFound`]. A missing path is never an error here; anything
//! that *is* an error carries the path it happened on.
//!
//! Deciding what a file's contents mean is a separate concern, and lives in
//! [`content`].

pub mod content;

use crate::error::{Error, Result};
use std::{
    fs::{File, metadata, read_to_string},
    io::{ErrorKind, Write},
    path::Path,
};
use tempfile::NamedTempFile;

/// What kind of entry a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    /// A socket, device, or anything else that is neither of the above.
    Other,
}

/// Reads `path`, or returns `None` when nothing is there.
///
/// # Errors
///
/// Returns an error if `path` exists but cannot be read.
pub fn read(path: &Path) -> Result<Option<String>> {
    match read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::read_file(path, error)),
    }
}

/// Reads `path` as text, or returns `None` when it is missing or is not UTF-8.
///
/// Use this for files that are only meaningful as short text, where binary
/// contents are an answer rather than a failure.
///
/// # Errors
///
/// Returns an error if `path` exists as text but cannot be read.
pub fn read_text(path: &Path) -> Result<Option<String>> {
    match read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) => {
            Ok(None)
        }
        Err(error) => Err(Error::read_file(path, error)),
    }
}

/// Reports what `path` names, or `None` when nothing is there.
///
/// # Errors
///
/// Returns an error if `path` cannot be inspected.
pub fn entry_kind(path: &Path) -> Result<Option<EntryKind>> {
    match metadata(path) {
        Ok(entry) if entry.is_dir() => Ok(Some(EntryKind::Directory)),
        Ok(entry) if entry.is_file() => Ok(Some(EntryKind::File)),
        Ok(_) => Ok(Some(EntryKind::Other)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::read_file(path, error)),
    }
}

/// Reports whether `path` exists.
///
/// # Errors
///
/// Returns an error if `path` cannot be inspected.
pub fn exists(path: &Path) -> Result<bool> {
    path.try_exists()
        .map_err(|source| Error::read_file(path, source))
}

/// Reports whether `path` is a directory. A missing path is not.
///
/// # Errors
///
/// Returns an error if `path` cannot be inspected.
pub fn is_dir(path: &Path) -> Result<bool> {
    Ok(entry_kind(path)? == Some(EntryKind::Directory))
}

/// Replaces `path` with `contents`, creating the directories above it.
///
/// The write goes to a temporary file in the destination directory and is then
/// renamed over `path`, so a reader never observes a partial file and an
/// interrupted write leaves the previous contents intact.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be
/// written, flushed, or renamed into place.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|source| Error::write_file(parent, source))?;

    let mut temp =
        NamedTempFile::new_in(parent).map_err(|source| Error::write_file(path, source))?;
    temp.write_all(contents.as_bytes())
        .and_then(|()| temp.as_file_mut().sync_all())
        .map_err(|source| Error::write_file(path, source))?;
    temp.persist(path)
        .map_err(|error| Error::write_file(path, error.error))?;

    // The rename is only durable once the directory entry itself is on disk.
    sync_dir(parent).map_err(|source| Error::write_file(parent, source))
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_ok, assert_ok_eq, assert_some_eq};
    use std::fs::{create_dir_all, write};
    use tempfile::TempDir;

    #[test]
    fn a_missing_file_reads_as_nothing() {
        let temp = TempDir::new().expect("create temp dir");

        assert_none!(assert_ok!(read(&temp.path().join("absent"))));
    }

    #[test]
    fn a_missing_entry_names_nothing() {
        let temp = TempDir::new().expect("create temp dir");

        assert_ok_eq!(entry_kind(&temp.path().join("absent")), None);
        assert_ok_eq!(exists(&temp.path().join("absent")), false);
        assert_ok_eq!(is_dir(&temp.path().join("absent")), false);
    }

    #[test]
    fn entries_are_told_apart() {
        let temp = TempDir::new().expect("create temp dir");
        let file = temp.path().join("file");
        write(&file, "contents").expect("write file");

        assert_ok_eq!(entry_kind(temp.path()), Some(EntryKind::Directory));
        assert_ok_eq!(entry_kind(&file), Some(EntryKind::File));
        assert_ok_eq!(is_dir(temp.path()), true);
        assert_ok_eq!(is_dir(&file), false);
    }

    #[cfg(unix)]
    #[test]
    fn special_files_are_neither_files_nor_directories() {
        use std::os::unix::net::UnixListener;

        let temp = TempDir::new().expect("create temp dir");
        let socket = temp.path().join("socket");
        let _listener = UnixListener::bind(&socket).expect("create socket");

        assert_ok_eq!(entry_kind(&socket), Some(EntryKind::Other));
    }

    #[cfg(unix)]
    #[test]
    fn binary_contents_are_not_text() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("binary");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).expect("write bytes");

        assert_none!(assert_ok!(read_text(&path)));
    }

    #[test]
    fn writing_creates_the_directories_above_the_file() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("nested/deeper/file.toml");

        assert_ok!(write_atomic(&path, "value = 1\n"));

        assert_some_eq!(assert_ok!(read(&path)), "value = 1\n".to_owned());
    }

    #[test]
    fn writing_replaces_existing_contents() {
        let temp = TempDir::new().expect("create temp dir");
        let path = temp.path().join("file.toml");
        create_dir_all(temp.path()).expect("create dir");
        write(&path, "old").expect("write original");

        assert_ok!(write_atomic(&path, "new"));

        assert_some_eq!(assert_ok!(read(&path)), "new".to_owned());
    }
}
