use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use triad_core::TriadError;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotAdapter;

impl SnapshotAdapter {
    pub fn collect(
        repo_root: &Utf8Path,
        include: &[String],
    ) -> Result<BTreeMap<String, String>, TriadError> {
        let mut files = Vec::new();
        walk(repo_root, repo_root, &mut files)?;
        files.sort();

        let mut snapshot = BTreeMap::new();
        for relative in files {
            if include
                .iter()
                .any(|pattern| matches_pattern(pattern, &relative))
            {
                let digest =
                    sha256_prefixed_hex(&fs::read(repo_root.join(&relative)).map_err(|err| {
                        TriadError::Io(format!(
                            "failed to read snapshot file {}: {err}",
                            repo_root.join(&relative)
                        ))
                    })?);
                snapshot.insert(relative, digest);
            }
        }

        Ok(snapshot)
    }
}

fn walk(root: &Utf8Path, current: &Utf8Path, files: &mut Vec<String>) -> Result<(), TriadError> {
    let mut entries = fs::read_dir(current)
        .map_err(|err| TriadError::Io(format!("failed to read directory {current}: {err}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            TriadError::Io(format!(
                "failed to read directory entry in {current}: {err}"
            ))
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
            TriadError::InvalidState(format!(
                "snapshot path is not valid UTF-8: {}",
                path.display()
            ))
        })?;
        let file_type = entry
            .file_type()
            .map_err(|err| TriadError::Io(format!("failed to read file type for {path}: {err}")))?;

        if file_type.is_dir() {
            walk(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|_| {
                TriadError::InvalidState(format!(
                    "snapshot path escaped repo root {}: {}",
                    root, path
                ))
            })?;
            files.push(relative.as_str().to_string());
        }
    }

    Ok(())
}

fn matches_pattern(pattern: &str, relative_path: &str) -> bool {
    let pattern_segments = pattern.split('/').collect::<Vec<_>>();
    let path_segments = relative_path.split('/').collect::<Vec<_>>();
    matches_segments(&pattern_segments, &path_segments)
}

fn matches_segments(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((&"**", rest)), _) => {
            matches_segments(rest, path)
                || path
                    .first()
                    .is_some_and(|_| matches_segments(pattern, &path[1..]))
        }
        (Some((segment, rest_pattern)), Some((path_segment, rest_path))) => {
            matches_segment(segment, path_segment) && matches_segments(rest_pattern, rest_path)
        }
        _ => false,
    }
}

fn matches_segment(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut remainder = value;
    let mut first = true;
    for part in parts.iter().filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            if let Some(next) = remainder.strip_prefix(part) {
                remainder = next;
            } else {
                return false;
            }
        } else if let Some(index) = remainder.find(part) {
            remainder = &remainder[index + part.len()..];
        } else {
            return false;
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
}

fn sha256_prefixed_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;

    use super::SnapshotAdapter;

    fn temp_dir(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "triad-fs-snapshot-{label}-{}-{unique}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(path).expect("utf8 temp path")
    }

    #[test]
    fn collect_snapshot_is_deterministic_for_unsorted_tree() {
        let repo_root = temp_dir("deterministic");
        fs::create_dir_all(repo_root.join("src")).expect("src dir should create");
        fs::create_dir_all(repo_root.join("tests")).expect("tests dir should create");
        fs::write(repo_root.join("tests/b.txt"), "b").expect("file should write");
        fs::write(repo_root.join("src/a.txt"), "a").expect("file should write");
        fs::write(repo_root.join("Cargo.toml"), "workspace").expect("file should write");

        let include = vec!["tests/**".into(), "src/**".into(), "Cargo.toml".into()];
        let first =
            SnapshotAdapter::collect(&repo_root, &include).expect("snapshot should collect");
        let second =
            SnapshotAdapter::collect(&repo_root, &include).expect("snapshot should collect");

        assert_eq!(first, second);
        assert_eq!(
            first.keys().cloned().collect::<Vec<_>>(),
            vec![
                "Cargo.toml".to_string(),
                "src/a.txt".to_string(),
                "tests/b.txt".to_string()
            ]
        );
    }
}
