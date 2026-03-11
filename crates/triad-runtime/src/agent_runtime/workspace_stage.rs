use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use triad_core::{RunId, TriadError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceStage {
    repo_root: Utf8PathBuf,
    workspace_root: Utf8PathBuf,
    baseline: BTreeMap<Utf8PathBuf, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceChange {
    pub path: Utf8PathBuf,
    pub kind: WorkspaceChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceChangeKind {
    Added,
    Modified,
    Removed,
}

pub(crate) fn stage_workspace(
    repo_root: &Utf8Path,
    state_dir: &Utf8Path,
    run_id: &RunId,
) -> Result<WorkspaceStage, TriadError> {
    let workspace_root = state_dir.join("tmp/workspaces").join(run_id.as_str());
    if workspace_root.exists() {
        fs::remove_dir_all(workspace_root.as_std_path()).map_err(|err| {
            TriadError::Io(format!(
                "failed to reset staged workspace {}: {err}",
                workspace_root
            ))
        })?;
    }
    fs::create_dir_all(workspace_root.as_std_path()).map_err(|err| {
        TriadError::Io(format!(
            "failed to create staged workspace {}: {err}",
            workspace_root
        ))
    })?;

    let _ = state_dir;
    copy_repo_tree(repo_root, &workspace_root)?;
    let baseline = snapshot_tree(&workspace_root)?;

    Ok(WorkspaceStage {
        repo_root: repo_root.to_path_buf(),
        workspace_root,
        baseline,
    })
}

impl WorkspaceStage {
    pub fn workspace_root(&self) -> &Utf8Path {
        &self.workspace_root
    }

    pub fn changed_paths(&self) -> Result<Vec<WorkspaceChange>, TriadError> {
        let current = snapshot_tree(&self.workspace_root)?;
        let mut paths = self
            .baseline
            .keys()
            .chain(current.keys())
            .cloned()
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();

        let mut changes = Vec::new();
        for path in paths {
            match (self.baseline.get(&path), current.get(&path)) {
                (None, Some(_)) => changes.push(WorkspaceChange {
                    path,
                    kind: WorkspaceChangeKind::Added,
                }),
                (Some(_), None) => changes.push(WorkspaceChange {
                    path,
                    kind: WorkspaceChangeKind::Removed,
                }),
                (Some(before), Some(after)) if before != after => changes.push(WorkspaceChange {
                    path,
                    kind: WorkspaceChangeKind::Modified,
                }),
                _ => {}
            }
        }

        Ok(changes)
    }

    pub fn apply_changes(&self, changes: &[WorkspaceChange]) -> Result<(), TriadError> {
        for change in changes {
            let repo_path = self.repo_root.join(&change.path);
            match change.kind {
                WorkspaceChangeKind::Added | WorkspaceChangeKind::Modified => {
                    let staged_path = self.workspace_root.join(&change.path);
                    if let Some(parent) = repo_path.parent() {
                        fs::create_dir_all(parent.as_std_path()).map_err(|err| {
                            TriadError::Io(format!(
                                "failed to create repo parent {}: {err}",
                                parent
                            ))
                        })?;
                    }
                    fs::copy(staged_path.as_std_path(), repo_path.as_std_path()).map_err(
                        |err| {
                            TriadError::Io(format!(
                                "failed to copy staged file {} to repo: {err}",
                                change.path
                            ))
                        },
                    )?;
                }
                WorkspaceChangeKind::Removed => {
                    if repo_path.exists() {
                        fs::remove_file(repo_path.as_std_path()).map_err(|err| {
                            TriadError::Io(format!(
                                "failed to remove repo file {} during copy-back: {err}",
                                change.path
                            ))
                        })?;
                    }
                }
            }
        }

        Ok(())
    }
}

fn copy_repo_tree(repo_root: &Utf8Path, workspace_root: &Utf8Path) -> Result<(), TriadError> {
    copy_tree_recursive(
        repo_root.as_std_path(),
        repo_root.as_std_path(),
        workspace_root.as_std_path(),
    )
}

fn copy_tree_recursive(
    root: &Path,
    current: &Path,
    workspace_root: &Path,
) -> Result<(), TriadError> {
    for entry in fs::read_dir(current).map_err(|err| {
        TriadError::Io(format!(
            "failed to read repo directory {}: {err}",
            current.display()
        ))
    })? {
        let entry =
            entry.map_err(|err| TriadError::Io(format!("failed to read repo entry: {err}")))?;
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(root).map_err(|err| {
            TriadError::InvalidState(format!(
                "failed to strip repo root {} from {}: {err}",
                root.display(),
                entry_path.display()
            ))
        })?;

        if should_skip(relative) {
            continue;
        }

        let destination = workspace_root.join(relative);
        let file_type = entry.file_type().map_err(|err| {
            TriadError::Io(format!(
                "failed to read file type for {}: {err}",
                entry_path.display()
            ))
        })?;

        if file_type.is_dir() {
            fs::create_dir_all(&destination).map_err(|err| {
                TriadError::Io(format!(
                    "failed to create staged directory {}: {err}",
                    destination.display()
                ))
            })?;
            copy_tree_recursive(root, &entry_path, workspace_root)?;
            continue;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                TriadError::Io(format!(
                    "failed to create staged parent {}: {err}",
                    parent.display()
                ))
            })?;
        }
        fs::copy(&entry_path, &destination).map_err(|err| {
            TriadError::Io(format!(
                "failed to stage repo file {}: {err}",
                entry_path.display()
            ))
        })?;
    }

    Ok(())
}

fn should_skip(relative: &Path) -> bool {
    let relative_utf8 = match Utf8Path::from_path(relative) {
        Some(path) => path,
        None => return false,
    };

    if relative_utf8 == Utf8Path::new(".git") || relative_utf8.starts_with(".git/") {
        return true;
    }

    if relative_utf8 == Utf8Path::new("target") || relative_utf8.starts_with("target/") {
        return true;
    }

    let skipped_relative = Utf8Path::new(".triad/tmp/workspaces");
    relative_utf8 == skipped_relative || relative_utf8.starts_with(skipped_relative)
}

fn snapshot_tree(root: &Utf8Path) -> Result<BTreeMap<Utf8PathBuf, String>, TriadError> {
    let mut snapshot = BTreeMap::new();
    snapshot_recursive(root.as_std_path(), root.as_std_path(), &mut snapshot)?;
    Ok(snapshot)
}

fn snapshot_recursive(
    root: &Path,
    current: &Path,
    snapshot: &mut BTreeMap<Utf8PathBuf, String>,
) -> Result<(), TriadError> {
    for entry in fs::read_dir(current).map_err(|err| {
        TriadError::Io(format!(
            "failed to read staged directory {}: {err}",
            current.display()
        ))
    })? {
        let entry =
            entry.map_err(|err| TriadError::Io(format!("failed to read staged entry: {err}")))?;
        let entry_path = entry.path();
        let relative = entry_path.strip_prefix(root).map_err(|err| {
            TriadError::InvalidState(format!(
                "failed to strip staged root {} from {}: {err}",
                root.display(),
                entry_path.display()
            ))
        })?;
        if should_skip(relative) {
            continue;
        }
        let file_type = entry.file_type().map_err(|err| {
            TriadError::Io(format!(
                "failed to read staged file type {}: {err}",
                entry_path.display()
            ))
        })?;

        if file_type.is_dir() {
            snapshot_recursive(root, &entry_path, snapshot)?;
            continue;
        }

        let bytes = fs::read(&entry_path).map_err(|err| {
            TriadError::Io(format!(
                "failed to read staged file {}: {err}",
                entry_path.display()
            ))
        })?;
        let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
        let relative = Utf8PathBuf::from_path_buf(relative.to_path_buf()).map_err(|path| {
            TriadError::InvalidState(format!(
                "staged path is not valid UTF-8: {}",
                path.display()
            ))
        })?;
        snapshot.insert(relative, digest);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use triad_core::RunId;

    use super::{WorkspaceChangeKind, stage_workspace};

    #[test]
    fn workspace_stage_detects_modified_added_and_removed_paths() {
        let repo = temp_dir("workspace-stage-diff");
        fs::create_dir_all(repo.join("src")).expect("src should exist");
        fs::write(repo.join("src/auth.rs"), "pub fn login() {}\n").expect("file should write");
        fs::write(repo.join("README.md"), "hello\n").expect("file should write");
        let state_dir = repo.join(".triad");
        fs::create_dir_all(&state_dir).expect("state dir should exist");

        let stage = stage_workspace(
            &Utf8PathBuf::from_path_buf(repo.clone()).expect("repo should be valid UTF-8"),
            &Utf8PathBuf::from_path_buf(state_dir).expect("state should be valid UTF-8"),
            &RunId::new("RUN-000001").expect("run id should parse"),
        )
        .expect("stage should build");

        fs::write(
            stage.workspace_root().join("src/auth.rs").as_std_path(),
            "pub fn login() -> bool { true }\n",
        )
        .expect("file should update");
        fs::write(
            stage.workspace_root().join("src/new.rs").as_std_path(),
            "pub fn new_file() {}\n",
        )
        .expect("new file should write");
        fs::remove_file(stage.workspace_root().join("README.md").as_std_path())
            .expect("file should remove");

        let changes = stage.changed_paths().expect("changes should compute");
        assert_eq!(changes.len(), 3);
        assert!(
            changes.iter().any(|change| change.path == "src/auth.rs"
                && change.kind == WorkspaceChangeKind::Modified)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.path == "src/new.rs"
                    && change.kind == WorkspaceChangeKind::Added)
        );
        assert!(changes.iter().any(
            |change| change.path == "README.md" && change.kind == WorkspaceChangeKind::Removed
        ));
    }

    #[test]
    fn workspace_stage_apply_changes_copies_back_allowed_files() {
        let repo = temp_dir("workspace-stage-copy-back");
        fs::create_dir_all(repo.join("src")).expect("src should exist");
        fs::write(repo.join("src/auth.rs"), "pub fn login() {}\n").expect("file should write");
        let state_dir = repo.join(".triad");
        fs::create_dir_all(&state_dir).expect("state dir should exist");

        let stage = stage_workspace(
            &Utf8PathBuf::from_path_buf(repo.clone()).expect("repo should be valid UTF-8"),
            &Utf8PathBuf::from_path_buf(state_dir).expect("state should be valid UTF-8"),
            &RunId::new("RUN-000001").expect("run id should parse"),
        )
        .expect("stage should build");

        fs::write(
            stage.workspace_root().join("src/auth.rs").as_std_path(),
            "pub fn login() -> bool { true }\n",
        )
        .expect("file should update");

        let changes = stage.changed_paths().expect("changes should compute");
        stage
            .apply_changes(&changes)
            .expect("copy-back should succeed");

        assert_eq!(
            fs::read_to_string(repo.join("src/auth.rs")).expect("repo file should read"),
            "pub fn login() -> bool { true }\n"
        );
    }

    #[test]
    fn workspace_stage_changed_paths_ignores_target_outputs() {
        let repo = temp_dir("workspace-stage-target-ignore");
        fs::create_dir_all(repo.join("src")).expect("src should exist");
        fs::write(repo.join("src/auth.rs"), "pub fn login() {}\n").expect("file should write");
        let state_dir = repo.join(".triad");
        fs::create_dir_all(&state_dir).expect("state dir should exist");

        let stage = stage_workspace(
            &Utf8PathBuf::from_path_buf(repo.clone()).expect("repo should be valid UTF-8"),
            &Utf8PathBuf::from_path_buf(state_dir).expect("state should be valid UTF-8"),
            &RunId::new("RUN-000001").expect("run id should parse"),
        )
        .expect("stage should build");

        let target_dir = stage.workspace_root().join("target/debug");
        fs::create_dir_all(target_dir.as_std_path()).expect("target dir should exist");
        fs::write(
            target_dir.join("triad-build.log").as_std_path(),
            "compiled\n",
        )
        .expect("target file should write");

        let changes = stage.changed_paths().expect("changes should compute");
        assert!(changes.is_empty());
    }

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "triad-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }
}
