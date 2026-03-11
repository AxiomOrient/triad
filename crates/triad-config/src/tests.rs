use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use camino::Utf8PathBuf;

use super::{
    AgentBackend, AgentConfig, CodexBackendConfig, GuardrailConfig, PathConfig, TriadConfig,
    VerifyConfig, discover_repo_root,
};

const VALID_CONFIG: &str = r#"version = 1

[paths]
claim_dir = "spec/claims"
docs_dir = "docs"
state_dir = ".triad"
evidence_file = ".triad/evidence.ndjson"
patch_dir = ".triad/patches"
run_dir = ".triad/runs"
schema_dir = "schemas"

[agent]
backend = "codex"
model = "gpt-5-codex"
effort = "medium"
approval_policy = "never"
sandbox_policy = "workspace-write"
timeout_seconds = 600

[agent.codex]
use_oss = false

[verify]
default_layers = ["unit", "contract", "integration"]
full_workspace_after_accept = true

[guardrails]
forbid_direct_spec_edits = true
forbid_git_commit = true
forbid_git_push = true
forbid_destructive_rm = true
"#;

#[test]
fn config_load_from_toml_str_deserializes_valid_config() {
    let config = TriadConfig::from_toml_str(VALID_CONFIG).expect("config should deserialize");

    assert_eq!(config.version, 1);
    assert_eq!(config.paths.claim_dir.as_str(), "spec/claims");
    assert_eq!(config.paths.schema_dir.as_str(), "schemas");
    assert_eq!(config.agent.backend, AgentBackend::Codex);
    assert_eq!(config.agent.codex, Some(CodexBackendConfig::default()));
    assert_eq!(config.agent.timeout_seconds, 600);
    assert_eq!(
        config.verify.default_layers,
        vec!["unit", "contract", "integration"]
    );
    assert!(config.guardrails.forbid_direct_spec_edits);
}

#[test]
fn bootstrap_config_roundtrips_and_validates_for_init() {
    let temp = TestDir::new("bootstrap-config");
    let repo_root = temp.path();

    let rendered = TriadConfig::bootstrap_toml().expect("bootstrap config should serialize");
    let canonical = TriadConfig::from_toml_str(&rendered)
        .expect("bootstrap config should deserialize")
        .canonicalize(repo_root)
        .expect("bootstrap paths should canonicalize")
        .validate_for_init()
        .expect("bootstrap config should validate for init");

    assert_eq!(
        canonical.paths.claim_dir,
        utf8(repo_root.join("spec/claims"))
    );
    assert_eq!(canonical.paths.docs_dir, utf8(repo_root.join("docs")));
    assert_eq!(canonical.paths.schema_dir, utf8(repo_root.join("schemas")));
}

#[test]
fn config_load_from_file_reads_and_deserializes_valid_config() {
    let temp = TestDir::new("config-load");
    let config_path = temp.path().join("triad.toml");
    fs::write(&config_path, VALID_CONFIG).expect("config fixture should be written");

    let config = TriadConfig::from_file(&config_path).expect("config file should load");

    assert_eq!(config.paths.state_dir.as_str(), ".triad");
    assert_eq!(
        config.paths.evidence_file.as_str(),
        ".triad/evidence.ndjson"
    );
    assert_eq!(config.agent.model, "gpt-5-codex");
    assert_eq!(config.agent.effort, "medium");
    assert!(config.verify.full_workspace_after_accept);
    assert!(config.guardrails.forbid_git_commit);
}

#[test]
fn backend_contract_supports_public_backend_names_only() {
    let codex = TriadConfig::from_toml_str(VALID_CONFIG).expect("codex config should parse");
    assert_eq!(codex.agent.backend, AgentBackend::Codex);
    let claude = TriadConfig::from_toml_str(
        &VALID_CONFIG
            .replace("backend = \"codex\"", "backend = \"claude\"")
            .replace("[agent.codex]\n", "[agent.claude]\n")
            .replace("gpt-5-codex", "claude-sonnet-4"),
    )
    .expect("claude config should parse");
    assert_eq!(claude.agent.backend, AgentBackend::Claude);

    let gemini = TriadConfig::from_toml_str(
        &VALID_CONFIG
            .replace("backend = \"codex\"", "backend = \"gemini\"")
            .replace("[agent.codex]\n", "[agent.gemini]\n")
            .replace("gpt-5-codex", "gemini-2.5-pro"),
    )
    .expect("gemini config should parse");
    assert_eq!(gemini.agent.backend, AgentBackend::Gemini);
}

#[test]
fn backend_contract_rejects_removed_coclai_alias() {
    let error = TriadConfig::from_toml_str(
        &VALID_CONFIG.replace("backend = \"codex\"", "backend = \"coclai\""),
    )
    .expect_err("removed alias should fail");

    assert!(
        error
            .to_string()
            .contains("unknown variant `coclai`, expected one of `codex`, `claude`, `gemini`")
    );
}

#[test]
fn backend_contract_rejects_backend_specific_section_mismatches() {
    let temp = TestDir::new("backend-contract-mismatch");
    let repo_root = temp.path();
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::create_dir_all(repo_root.join("schemas")).expect("schema dir should exist");

    let config = TriadConfig::from_toml_str(
        r#"version = 1

[paths]
claim_dir = "spec/claims"
docs_dir = "docs"
state_dir = ".triad"
evidence_file = ".triad/evidence.ndjson"
patch_dir = ".triad/patches"
run_dir = ".triad/runs"
schema_dir = "schemas"

[agent]
backend = "codex"
model = "gpt-5-codex"
effort = "medium"
approval_policy = "never"
sandbox_policy = "workspace-write"
timeout_seconds = 600

[agent.claude]
permission_mode = "acceptEdits"

[verify]
default_layers = ["unit", "contract", "integration"]
full_workspace_after_accept = true

[guardrails]
forbid_direct_spec_edits = true
forbid_git_commit = true
forbid_git_push = true
forbid_destructive_rm = true
"#,
    )
    .expect("config should deserialize");

    let error = config
        .canonicalize(repo_root)
        .expect_err("mismatched backend-specific section should fail");

    assert_eq!(
        error.to_string(),
        "config error: invalid config agent.claude: is only allowed when agent.backend = \"claude\""
    );
}

#[test]
fn repo_root_discovery_finds_same_root_from_nested_directory() {
    let temp = TestDir::new("repo-root");
    let repo_root = temp.path();
    let nested = repo_root.join("crates/triad-cli/src");
    fs::create_dir_all(&nested).expect("nested directory should be created");
    fs::write(repo_root.join("triad.toml"), VALID_CONFIG).expect("config should be written");

    let discovered = discover_repo_root(&nested).expect("repo root should be found");

    assert_eq!(discovered, repo_root);
}

#[test]
fn repo_root_discovery_accepts_file_path_start() {
    let temp = TestDir::new("repo-root-file");
    let repo_root = temp.path();
    let nested_file = repo_root.join("crates/triad-cli/src/main.rs");
    fs::create_dir_all(nested_file.parent().expect("file should have parent"))
        .expect("nested directory should be created");
    fs::write(repo_root.join("triad.toml"), VALID_CONFIG).expect("config should be written");
    fs::write(&nested_file, "fn main() {}\n").expect("source file should be written");

    let discovered = discover_repo_root(&nested_file).expect("repo root should be found");

    assert_eq!(discovered, repo_root);
}

#[test]
fn repo_root_discovery_loads_config_from_discovered_root() {
    let temp = TestDir::new("repo-root-config");
    let repo_root = temp.path();
    let nested = repo_root.join("spec/claims");
    fs::create_dir_all(&nested).expect("nested directory should be created");
    fs::write(repo_root.join("triad.toml"), VALID_CONFIG).expect("config should be written");

    let discovered = discover_repo_root(&nested).expect("repo root should be found");
    let config =
        TriadConfig::from_repo_root(&discovered).expect("config should load from repo root");

    assert_eq!(config.paths.claim_dir.as_str(), "spec/claims");
    assert_eq!(config.paths.run_dir.as_str(), ".triad/runs");
}

#[test]
fn path_canonicalization_anchors_relative_paths_to_repo_root() {
    let temp = TestDir::new("canonical-relative");
    let repo_root = temp.path();
    let config_path = repo_root.join("triad.toml");
    fs::write(&config_path, VALID_CONFIG).expect("config should be written");

    let canonical = TriadConfig::from_file(&config_path)
        .expect("config should load")
        .canonicalize(repo_root)
        .expect("paths should canonicalize");

    assert_eq!(
        canonical.paths.claim_dir,
        utf8(repo_root.join("spec/claims"))
    );
    assert_eq!(canonical.paths.state_dir, utf8(repo_root.join(".triad")));
    assert_eq!(canonical.paths.schema_dir, utf8(repo_root.join("schemas")));
    assert_eq!(
        canonical.paths.evidence_file,
        utf8(repo_root.join(".triad/evidence.ndjson"))
    );
}

#[test]
fn path_canonicalization_normalizes_relative_segments() {
    let temp = TestDir::new("canonical-segments");
    let repo_root = temp.path();
    let config = TriadConfig {
        version: 1,
        paths: PathConfig {
            claim_dir: Utf8PathBuf::from("./spec/claims/../claims"),
            docs_dir: Utf8PathBuf::from("docs"),
            state_dir: Utf8PathBuf::from(".triad/./state/.."),
            evidence_file: Utf8PathBuf::from(".triad/runs/../evidence.ndjson"),
            patch_dir: Utf8PathBuf::from(".triad/patches"),
            run_dir: Utf8PathBuf::from(".triad/runs"),
            schema_dir: Utf8PathBuf::from("schemas/../schemas"),
        },
        agent: test_agent_config(),
        verify: test_verify_config(),
        guardrails: test_guardrails(),
    };

    let canonical = config
        .canonicalize(repo_root)
        .expect("paths should canonicalize");

    assert_eq!(
        canonical.paths.claim_dir,
        utf8(repo_root.join("spec/claims"))
    );
    assert_eq!(canonical.paths.state_dir, utf8(repo_root.join(".triad")));
    assert_eq!(
        canonical.paths.evidence_file,
        utf8(repo_root.join(".triad/evidence.ndjson"))
    );
    assert_eq!(canonical.paths.schema_dir, utf8(repo_root.join("schemas")));
}

#[test]
fn path_canonicalization_preserves_absolute_inputs() {
    let temp = TestDir::new("canonical-absolute");
    let repo_root = temp.path();
    let external_root = temp.path().join("external-root");
    let config = TriadConfig {
        version: 1,
        paths: PathConfig {
            claim_dir: utf8(external_root.join("spec/claims")),
            docs_dir: utf8(external_root.join("docs")),
            state_dir: utf8(external_root.join(".triad")),
            evidence_file: utf8(external_root.join(".triad/evidence.ndjson")),
            patch_dir: utf8(external_root.join(".triad/patches")),
            run_dir: utf8(external_root.join(".triad/runs")),
            schema_dir: utf8(external_root.join("schemas")),
        },
        agent: test_agent_config(),
        verify: test_verify_config(),
        guardrails: test_guardrails(),
    };

    let canonical = config
        .canonicalize(repo_root)
        .expect("absolute paths should stay absolute");

    assert_eq!(
        canonical.paths.claim_dir,
        utf8(external_root.join("spec/claims"))
    );
    assert_eq!(
        canonical.paths.state_dir,
        utf8(external_root.join(".triad"))
    );
    assert_eq!(
        canonical.paths.schema_dir,
        utf8(external_root.join("schemas"))
    );
}

#[test]
fn config_validation_rejects_empty_claim_dir() {
    let temp = TestDir::new("config-validation-empty-claim");
    let repo_root = temp.path();
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::create_dir_all(repo_root.join("schemas")).expect("schema dir should exist");

    let config = TriadConfig {
        version: 1,
        paths: PathConfig {
            claim_dir: Utf8PathBuf::from(""),
            docs_dir: Utf8PathBuf::from("docs"),
            state_dir: Utf8PathBuf::from(".triad"),
            evidence_file: Utf8PathBuf::from(".triad/evidence.ndjson"),
            patch_dir: Utf8PathBuf::from(".triad/patches"),
            run_dir: Utf8PathBuf::from(".triad/runs"),
            schema_dir: Utf8PathBuf::from("schemas"),
        },
        agent: test_agent_config(),
        verify: test_verify_config(),
        guardrails: test_guardrails(),
    };

    let error = config
        .canonicalize(repo_root)
        .expect_err("empty claim dir should fail");

    assert_eq!(
        error.to_string(),
        "config error: invalid config paths.claim_dir: must not be empty"
    );
}

#[test]
fn config_validation_rejects_missing_schema_dir() {
    let temp = TestDir::new("config-validation-missing-schema");
    let repo_root = temp.path();
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");

    let error = TriadConfig::from_toml_str(VALID_CONFIG)
        .expect("config should deserialize")
        .canonicalize(repo_root)
        .expect("paths should canonicalize")
        .validate()
        .expect_err("missing schema dir should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "config error: invalid config paths.schema_dir: directory does not exist: {}",
            repo_root.join("schemas").display()
        )
    );
}

#[test]
fn config_validation_rejects_paths_outside_state_dir() {
    let temp = TestDir::new("config-validation-state-child");
    let repo_root = temp.path();
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::create_dir_all(repo_root.join("schemas")).expect("schema dir should exist");

    let config = TriadConfig {
        version: 1,
        paths: PathConfig {
            claim_dir: Utf8PathBuf::from("spec/claims"),
            docs_dir: Utf8PathBuf::from("docs"),
            state_dir: Utf8PathBuf::from(".triad"),
            evidence_file: Utf8PathBuf::from("evidence.ndjson"),
            patch_dir: Utf8PathBuf::from(".triad/patches"),
            run_dir: Utf8PathBuf::from(".triad/runs"),
            schema_dir: Utf8PathBuf::from("schemas"),
        },
        agent: test_agent_config(),
        verify: test_verify_config(),
        guardrails: test_guardrails(),
    };

    let error = config
        .canonicalize(repo_root)
        .expect("paths should canonicalize")
        .validate()
        .expect_err("evidence path outside state dir should fail");

    assert_eq!(
        error.to_string(),
        format!(
            "config error: invalid config paths.evidence_file: must stay under {}",
            repo_root.join(".triad").display()
        )
    );
}

#[test]
fn config_validation_accepts_documented_layout() {
    let temp = TestDir::new("config-validation-valid");
    let repo_root = temp.path();
    fs::create_dir_all(repo_root.join("docs")).expect("docs dir should exist");
    fs::create_dir_all(repo_root.join("schemas")).expect("schema dir should exist");

    let canonical = TriadConfig::from_toml_str(VALID_CONFIG)
        .expect("config should deserialize")
        .canonicalize(repo_root)
        .expect("paths should canonicalize")
        .validate()
        .expect("documented config should validate");

    assert_eq!(canonical.paths.schema_dir, utf8(repo_root.join("schemas")));
    assert_eq!(canonical.paths.docs_dir, utf8(repo_root.join("docs")));
}

fn test_agent_config() -> AgentConfig {
    AgentConfig {
        backend: AgentBackend::Codex,
        model: "gpt-5-codex".into(),
        effort: "medium".into(),
        approval_policy: "never".into(),
        sandbox_policy: "workspace-write".into(),
        timeout_seconds: 600,
        codex: Some(CodexBackendConfig::default()),
        claude: None,
        gemini: None,
    }
}

fn test_verify_config() -> VerifyConfig {
    VerifyConfig {
        default_layers: vec!["unit".into(), "contract".into(), "integration".into()],
        full_workspace_after_accept: true,
    }
}

fn test_guardrails() -> GuardrailConfig {
    GuardrailConfig {
        forbid_direct_spec_edits: true,
        forbid_git_commit: true,
        forbid_git_push: true,
        forbid_destructive_rm: true,
    }
}

fn utf8(path: PathBuf) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(path).expect("test path should be valid UTF-8")
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        let path = env::temp_dir().join(format!("triad-config-{label}-{}-{unique}", process::id()));

        fs::create_dir_all(&path).expect("temp dir should be created");

        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
