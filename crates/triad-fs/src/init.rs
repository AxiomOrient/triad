use std::fs;

use camino::Utf8Path;
use triad_core::TriadError;

use crate::config::{CONFIG_FILE_NAME, TriadConfig};

pub fn init_scaffold(repo_root: &Utf8Path, force: bool) -> Result<(), TriadError> {
    let config = TriadConfig::bootstrap_defaults().canonicalize(repo_root)?;

    ensure_dir(&config.paths.claim_dir)?;
    ensure_parent_dir(&config.paths.evidence_file)?;
    ensure_text_file(
        &repo_root.join(CONFIG_FILE_NAME),
        &TriadConfig::bootstrap_toml()?,
        force,
    )?;
    ensure_file(&config.paths.evidence_file, force)?;

    Ok(())
}

fn ensure_dir(path: &Utf8Path) -> Result<(), TriadError> {
    fs::create_dir_all(path)
        .map_err(|err| TriadError::Io(format!("failed to create directory {}: {err}", path)))
}

fn ensure_parent_dir(path: &Utf8Path) -> Result<(), TriadError> {
    let parent = path
        .parent()
        .ok_or_else(|| TriadError::InvalidState(format!("path has no parent directory: {path}")))?;
    ensure_dir(parent)
}

fn ensure_text_file(path: &Utf8Path, contents: &str, force: bool) -> Result<(), TriadError> {
    if path.exists() && !force {
        return Ok(());
    }

    fs::write(path, contents)
        .map_err(|err| TriadError::Io(format!("failed to write file {}: {err}", path)))
}

fn ensure_file(path: &Utf8Path, force: bool) -> Result<(), TriadError> {
    if path.exists() && !force {
        return Ok(());
    }

    fs::write(path, "")
        .map_err(|err| TriadError::Io(format!("failed to create file {}: {err}", path)))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;

    use super::init_scaffold;

    fn temp_dir(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("triad-fs-{label}-{}-{unique}", process::id()));
        fs::create_dir_all(&path).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(path).expect("temp dir path should be utf8")
    }

    #[test]
    fn init_scaffold_creates_minimal_repo_state() {
        let repo_root = temp_dir("init");

        init_scaffold(&repo_root, false).expect("scaffold should succeed");

        assert!(repo_root.join("triad.toml").is_file());
        assert!(repo_root.join("spec/claims").is_dir());
        assert!(repo_root.join(".triad/evidence.ndjson").is_file());
    }

    #[test]
    fn init_scaffold_preserves_existing_config_without_force() {
        let repo_root = temp_dir("init-preserve");
        fs::write(repo_root.join("triad.toml"), "version = 99\n").expect("config should write");

        init_scaffold(&repo_root, false).expect("scaffold should succeed");

        assert_eq!(
            fs::read_to_string(repo_root.join("triad.toml")).expect("config should read"),
            "version = 99\n"
        );
    }

    #[test]
    fn init_scaffold_overwrites_existing_config_with_force() {
        let repo_root = temp_dir("init-force");
        fs::write(repo_root.join("triad.toml"), "version = 99\n").expect("config should write");

        init_scaffold(&repo_root, true).expect("scaffold should succeed");

        let updated = fs::read_to_string(repo_root.join("triad.toml")).expect("config should read");
        assert!(updated.contains("version = 2"));
        assert!(updated.contains("claim_dir = \"spec/claims\""));
    }
}
