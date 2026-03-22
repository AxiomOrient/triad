use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::TriadError;

use crate::config::{CONFIG_FILE_NAME, TriadConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
struct InitScaffoldPlan {
    steps: Vec<InitStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InitStep {
    Dir {
        path: Utf8PathBuf,
    },
    TextFile {
        path: Utf8PathBuf,
        contents: String,
        force: bool,
    },
    File {
        path: Utf8PathBuf,
        force: bool,
    },
}

pub fn init_scaffold(repo_root: &Utf8Path, force: bool) -> Result<(), TriadError> {
    let plan = plan_init_scaffold(repo_root, force)?;
    apply_init_scaffold(&plan)
}

fn plan_init_scaffold(repo_root: &Utf8Path, force: bool) -> Result<InitScaffoldPlan, TriadError> {
    let config = TriadConfig::bootstrap_defaults().canonicalize(repo_root)?;
    let config_path = repo_root.join(CONFIG_FILE_NAME);
    let evidence_parent = parent_dir(&config.paths.evidence_file)?;

    Ok(InitScaffoldPlan {
        steps: vec![
            InitStep::Dir {
                path: config.paths.claim_dir,
            },
            InitStep::Dir {
                path: evidence_parent,
            },
            InitStep::TextFile {
                path: config_path,
                contents: TriadConfig::bootstrap_toml()?,
                force,
            },
            InitStep::File {
                path: config.paths.evidence_file,
                force,
            },
        ],
    })
}

fn apply_init_scaffold(plan: &InitScaffoldPlan) -> Result<(), TriadError> {
    for step in &plan.steps {
        apply_init_step(step)?;
    }
    Ok(())
}

fn apply_init_step(step: &InitStep) -> Result<(), TriadError> {
    match step {
        InitStep::Dir { path } => ensure_dir(path),
        InitStep::TextFile {
            path,
            contents,
            force,
        } => ensure_text_file(path, contents, *force),
        InitStep::File { path, force } => ensure_file(path, *force),
    }
}

fn ensure_dir(path: &Utf8Path) -> Result<(), TriadError> {
    fs::create_dir_all(path)
        .map_err(|err| TriadError::Io(format!("failed to create directory {}: {err}", path)))
}

fn parent_dir(path: &Utf8Path) -> Result<Utf8PathBuf, TriadError> {
    path.parent()
        .map(Utf8Path::to_owned)
        .ok_or_else(|| TriadError::InvalidState(format!("path has no parent directory: {path}")))
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

    use crate::config::TriadConfig;

    use super::{InitStep, init_scaffold, plan_init_scaffold};

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

    #[test]
    fn plan_init_scaffold_is_deterministic_from_inputs() {
        let repo_root = Utf8PathBuf::from("/repo");

        let first = plan_init_scaffold(&repo_root, true).expect("plan should succeed");
        let second = plan_init_scaffold(&repo_root, true).expect("plan should succeed");

        assert_eq!(first, second);
        assert_eq!(
            first.steps,
            vec![
                InitStep::Dir {
                    path: Utf8PathBuf::from("/repo/spec/claims")
                },
                InitStep::Dir {
                    path: Utf8PathBuf::from("/repo/.triad")
                },
                InitStep::TextFile {
                    path: Utf8PathBuf::from("/repo/triad.toml"),
                    contents: TriadConfig::bootstrap_toml()
                        .expect("bootstrap toml should serialize"),
                    force: true
                },
                InitStep::File {
                    path: Utf8PathBuf::from("/repo/.triad/evidence.ndjson"),
                    force: true
                }
            ]
        );
    }
}
