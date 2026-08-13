//! File Manager. Responsible only for reading and writing files on disk.
//! It has no knowledge of the editor buffer's internal representation
//! beyond plain text in, plain text out.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub struct FileManager;

impl FileManager {
    /// Open an existing file, returning its contents. If the file does
    /// not exist, an empty string is returned so the caller can treat it
    /// as a fresh buffer for `shdev newfile.sh`-style workflows.
    pub fn open(path: &Path) -> Result<String> {
        if path.exists() {
            fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
        } else {
            Ok(String::new())
        }
    }

    /// Reload a file from disk (alias of `open`, kept distinct for
    /// clarity/intent at call sites).
    #[allow(dead_code)]
    pub fn reload(path: &Path) -> Result<String> {
        Self::open(path)
    }

    /// Write `contents` to `path`, creating parent directories if needed.
    pub fn save(path: &Path, contents: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
        }
        fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
    }

    /// Create a new, empty file at `path` if it doesn't already exist.
    #[allow(dead_code)]
    pub fn create(path: &Path) -> Result<()> {
        if !path.exists() {
            Self::save(path, "")?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn resolve(path_str: &str) -> PathBuf {
        PathBuf::from(path_str)
    }
}
