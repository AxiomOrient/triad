use std::fs::{self, File};
use std::path::Path;

use triad_core::TriadError;

pub(crate) fn ensure_dir(path: &Path) -> Result<(), TriadError> {
    if path.exists() {
        if path.is_dir() {
            return Ok(());
        }

        return Err(TriadError::InvalidState(format!(
            "expected directory at {}",
            path.display()
        )));
    }

    fs::create_dir_all(path).map_err(|err| {
        TriadError::Io(format!(
            "failed to create directory {}: {err}",
            path.display()
        ))
    })
}

pub(crate) fn ensure_file(path: &Path, force: bool) -> Result<(), TriadError> {
    if path.exists() {
        if path.is_file() {
            if force {
                File::create(path).map(|_| ()).map_err(|err| {
                    TriadError::Io(format!("failed to recreate file {}: {err}", path.display()))
                })?;
            }

            return Ok(());
        }

        return Err(TriadError::InvalidState(format!(
            "expected file at {}",
            path.display()
        )));
    }

    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    File::create(path)
        .map(|_| ())
        .map_err(|err| TriadError::Io(format!("failed to create file {}: {err}", path.display())))
}

pub(crate) fn ensure_text_file(path: &Path, contents: &str) -> Result<(), TriadError> {
    if path.exists() {
        if path.is_file() {
            return Ok(());
        }

        return Err(TriadError::InvalidState(format!(
            "expected file at {}",
            path.display()
        )));
    }

    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    fs::write(path, contents)
        .map_err(|err| TriadError::Io(format!("failed to write file {}: {err}", path.display())))
}

pub(crate) fn write_json_file(
    path: &Path,
    value: &serde_json::Value,
    label: &str,
) -> Result<(), TriadError> {
    let contents = serde_json::to_string_pretty(value).map_err(|err| {
        TriadError::InvalidState(format!(
            "failed to serialize {label} at {}: {err}",
            path.display()
        ))
    })?;
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    fs::write(path, contents)
        .map_err(|err| TriadError::Io(format!("failed to write {label} {}: {err}", path.display())))
}
