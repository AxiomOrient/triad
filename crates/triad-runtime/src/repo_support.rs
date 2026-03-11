use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{PatchId, RunId, TriadError};

pub(crate) fn unique_non_empty_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();

    for value in values {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() && !seen.contains(trimmed.as_str()) {
            seen.insert(trimmed.clone());
            unique.push(trimmed);
        }
    }

    unique
}

pub(crate) fn resolve_repo_relative_path(
    repo_root: &Path,
    path: &Utf8Path,
) -> Result<PathBuf, TriadError> {
    validate_repo_relative_path(path, "repo-relative path")?;
    Ok(repo_root.join(path.as_std_path()))
}

pub(crate) fn validate_repo_relative_path(path: &Utf8Path, kind: &str) -> Result<(), TriadError> {
    if path.is_absolute() {
        return Err(TriadError::InvalidState(format!(
            "{kind} must be relative to repo root: {}",
            path
        )));
    }

    if path
        .components()
        .any(|component| matches!(component, camino::Utf8Component::ParentDir))
    {
        return Err(TriadError::InvalidState(format!(
            "{kind} must not escape repo root: {}",
            path
        )));
    }

    Ok(())
}

pub(crate) fn repo_relative_utf8(repo_root: &Path, path: &Path) -> Result<Utf8PathBuf, TriadError> {
    let relative = path.strip_prefix(repo_root).map_err(|_| {
        TriadError::InvalidState(format!(
            "path is outside repo root {}: {}",
            repo_root.display(),
            path.display()
        ))
    })?;
    utf8_path(relative.to_path_buf(), "repo-relative path")
}

pub(crate) fn patch_json_path(patch_dir: &Path, id: &PatchId) -> PathBuf {
    patch_dir.join(format!("{}.json", id.as_str()))
}

pub(crate) fn patch_diff_path(patch_dir: &Path, id: &PatchId) -> PathBuf {
    patch_dir.join(format!("{}.diff", id.as_str()))
}

pub(crate) fn run_json_path(run_dir: &Path, id: &RunId) -> PathBuf {
    run_dir.join(format!("{}.json", id.as_str()))
}

pub(crate) fn sha256_prefixed_hex(bytes: &[u8]) -> String {
    format!("sha256:{}", digest_to_hex(bytes))
}

pub(crate) fn normalize_serde_row_error(message: &str) -> &str {
    message
        .split_once(" at line ")
        .map(|(head, _)| head)
        .unwrap_or(message)
}

pub(crate) fn utf8_path(path: PathBuf, context: &str) -> Result<Utf8PathBuf, TriadError> {
    Utf8PathBuf::from_path_buf(path).map_err(|path_buf| {
        TriadError::InvalidState(format!(
            "{context} is not valid UTF-8: {}",
            path_buf.display()
        ))
    })
}

fn digest_to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut hex, "{byte:02x}").expect("writing digest hex should not fail");
    }
    hex
}
