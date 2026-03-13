use std::{
    fs, process,
    time::{SystemTime, UNIX_EPOCH},
};

use camino::Utf8PathBuf;
use clap::{CommandFactory, Parser};

use crate::{cli::Cli, execute_cli_from_dir};

fn temp_dir(label: &str) -> Utf8PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("triad-cli-{label}-{}-{unique}", process::id()));
    fs::create_dir_all(&path).expect("test directory should be created");
    Utf8PathBuf::from_path_buf(path).expect("utf8 temp dir")
}

fn write_claim(repo_root: &Utf8PathBuf, claim_id: &str, title: &str) {
    let claim_dir = repo_root.join("spec/claims");
    fs::create_dir_all(&claim_dir).expect("claim dir should exist");
    fs::write(
        claim_dir.join(format!("{claim_id}.md")),
        format!(
            "# {claim_id} {title}\n\n## Claim\nSystem behavior.\n\n## Examples\n- valid -> 200\n\n## Invariants\n- invariant holds\n"
        ),
    )
    .expect("claim should write");
}

fn write_config(repo_root: &Utf8PathBuf, commands: &[&str]) {
    let commands = commands
        .iter()
        .map(|command| format!("  \"{command}\""))
        .collect::<Vec<_>>()
        .join(",\n");
    fs::write(
        repo_root.join("triad.toml"),
        format!(
            "version = 2\n\n[paths]\nclaim_dir = \"spec/claims\"\nevidence_file = \".triad/evidence.ndjson\"\n\n[snapshot]\ninclude = [\"spec/claims/**\"]\n\n[verify]\ncommands = [\n{commands}\n]\n"
        ),
    )
    .expect("config should write");
}

#[test]
fn cli_rejects_legacy_commands() {
    assert!(Cli::try_parse_from(["triad", "next"]).is_err());
    assert!(Cli::try_parse_from(["triad", "work"]).is_err());
    assert!(Cli::try_parse_from(["triad", "accept"]).is_err());
    assert!(Cli::try_parse_from(["triad", "agent"]).is_err());
}

#[test]
fn cli_help_lists_only_new_commands() {
    let help = Cli::command().render_long_help().to_string();

    assert!(help.contains("init"));
    assert!(help.contains("lint"));
    assert!(help.contains("verify"));
    assert!(help.contains("report"));
    assert!(!help.contains("work"));
    assert!(!help.contains("accept"));
    assert!(!help.contains("agent"));
}

#[test]
fn init_creates_minimal_scaffold() {
    let repo_root = temp_dir("init");
    let cli = Cli::try_parse_from(["triad", "init"]).expect("cli should parse");
    let mut stdout = Vec::new();

    let exit = execute_cli_from_dir(cli, &mut stdout, &repo_root).expect("init should succeed");

    assert_eq!(exit as u8, 0);
    assert!(repo_root.join("triad.toml").is_file());
    assert!(repo_root.join("spec/claims").is_dir());
    assert!(repo_root.join(".triad/evidence.ndjson").is_file());
}

#[test]
fn lint_json_reports_claims() {
    let repo_root = temp_dir("lint");
    write_config(&repo_root, &["true"]);
    write_claim(&repo_root, "REQ-auth-001", "Login success");
    let cli = Cli::try_parse_from(["triad", "lint", "--all", "--json"]).expect("cli should parse");
    let mut stdout = Vec::new();

    let exit = execute_cli_from_dir(cli, &mut stdout, &repo_root).expect("lint should succeed");
    let json: serde_json::Value = serde_json::from_slice(&stdout).expect("stdout should be json");

    assert_eq!(exit as u8, 0);
    assert_eq!(json["ok"], true);
    assert_eq!(json["claim_count"], 1);
    assert_eq!(json["claims"][0]["claim_id"], "REQ-auth-001");
    assert_eq!(json["claims"][0]["title"], "Login success");
    assert_eq!(json["verify_commands"], serde_json::json!(["true"]));
}

#[test]
fn verify_and_report_json_use_single_output_path() {
    let repo_root = temp_dir("verify");
    write_config(&repo_root, &["true"]);
    write_claim(&repo_root, "REQ-auth-001", "Login success");

    let verify_cli = Cli::try_parse_from(["triad", "verify", "--claim", "REQ-auth-001", "--json"])
        .expect("verify cli should parse");
    let mut verify_stdout = Vec::new();
    let verify_exit = execute_cli_from_dir(verify_cli, &mut verify_stdout, &repo_root)
        .expect("verify should succeed");
    let verify_json: serde_json::Value =
        serde_json::from_slice(&verify_stdout).expect("verify stdout should be json");

    assert_eq!(
        verify_json,
        serde_json::json!({
            "claim_id": "REQ-auth-001",
            "evidence_ids": ["EVID-000001"],
            "report": {
                "claim_id": "REQ-auth-001",
                "status": "confirmed",
                "reasons": ["fresh hard pass exists"],
                "fresh_evidence_ids": ["EVID-000001"],
                "stale_evidence_ids": [],
                "advisory_evidence_ids": [],
                "strongest_verdict": "pass"
            }
        })
    );
    assert_eq!(verify_exit as u8, 0);

    let report_cli = Cli::try_parse_from(["triad", "report", "--all", "--json"])
        .expect("report cli should parse");
    let mut report_stdout = Vec::new();
    let report_exit = execute_cli_from_dir(report_cli, &mut report_stdout, &repo_root)
        .expect("report should succeed");
    let report_json: serde_json::Value =
        serde_json::from_slice(&report_stdout).expect("report stdout should be json");

    assert_eq!(
        report_json,
        serde_json::json!([{
            "claim_id": "REQ-auth-001",
            "status": "confirmed",
            "reasons": ["fresh hard pass exists"],
            "fresh_evidence_ids": ["EVID-000001"],
            "stale_evidence_ids": [],
            "advisory_evidence_ids": [],
            "strongest_verdict": "pass"
        }])
    );
    assert_eq!(report_exit as u8, 0);
}

#[test]
fn verify_json_suppresses_verify_command_stdout() {
    let repo_root = temp_dir("verify-json-clean");
    write_config(
        &repo_root,
        &[
            "printf 'noise on stdout\\n'",
            "printf 'noise on stderr\\n' >&2",
        ],
    );
    write_claim(&repo_root, "REQ-auth-001", "Login success");

    let verify_cli = Cli::try_parse_from(["triad", "verify", "--claim", "REQ-auth-001", "--json"])
        .expect("verify cli should parse");
    let mut verify_stdout = Vec::new();

    let verify_exit = execute_cli_from_dir(verify_cli, &mut verify_stdout, &repo_root)
        .expect("verify should succeed");
    let verify_json: serde_json::Value =
        serde_json::from_slice(&verify_stdout).expect("verify stdout should remain json");

    assert_eq!(verify_exit as u8, 0);
    assert_eq!(verify_json["claim_id"], "REQ-auth-001");
    assert_eq!(verify_json["report"]["status"], "confirmed");
    assert_eq!(
        verify_json["evidence_ids"],
        serde_json::json!(["EVID-000001", "EVID-000002"])
    );
}

#[test]
fn report_all_json_uses_batch_verification_for_multiple_claims() {
    let repo_root = temp_dir("report-all");
    write_config(&repo_root, &["true"]);
    write_claim(&repo_root, "REQ-auth-001", "Login success");
    write_claim(&repo_root, "REQ-auth-002", "Logout success");

    let verify_cli = Cli::try_parse_from(["triad", "verify", "--claim", "REQ-auth-001"])
        .expect("verify cli should parse");
    let mut verify_stdout = Vec::new();

    let verify_exit = execute_cli_from_dir(verify_cli, &mut verify_stdout, &repo_root)
        .expect("verify should succeed");

    assert_eq!(verify_exit as u8, 0);

    let report_cli = Cli::try_parse_from(["triad", "report", "--all", "--json"])
        .expect("report cli should parse");
    let mut report_stdout = Vec::new();

    let report_exit = execute_cli_from_dir(report_cli, &mut report_stdout, &repo_root)
        .expect("report should succeed");
    let report_json: serde_json::Value =
        serde_json::from_slice(&report_stdout).expect("report stdout should be json");

    assert_eq!(
        report_json,
        serde_json::json!([
            {
                "claim_id": "REQ-auth-001",
                "status": "confirmed",
                "reasons": ["fresh hard pass exists"],
                "fresh_evidence_ids": ["EVID-000001"],
                "stale_evidence_ids": [],
                "advisory_evidence_ids": [],
                "strongest_verdict": "pass"
            },
            {
                "claim_id": "REQ-auth-002",
                "status": "unsupported",
                "reasons": ["no hard evidence exists"],
                "fresh_evidence_ids": [],
                "stale_evidence_ids": [],
                "advisory_evidence_ids": [],
                "strongest_verdict": null
            }
        ])
    );
    assert_eq!(report_exit as u8, 0);
}

#[test]
fn init_and_verify_human_output_are_stable() {
    let repo_root = temp_dir("human-output");
    let init_cli = Cli::try_parse_from(["triad", "init"]).expect("init cli should parse");
    let mut init_stdout = Vec::new();

    let init_exit =
        execute_cli_from_dir(init_cli, &mut init_stdout, &repo_root).expect("init should succeed");
    let init_output = String::from_utf8(init_stdout).expect("stdout should be utf8");

    assert_eq!(init_exit as u8, 0);
    assert_eq!(
        init_output,
        format!(
            "initialized triad scaffold\nConfig: {}\nEvidence: {}\n",
            repo_root.join("triad.toml"),
            repo_root.join(".triad/evidence.ndjson")
        )
    );

    write_config(&repo_root, &["true"]);
    write_claim(&repo_root, "REQ-auth-001", "Login success");
    let verify_cli = Cli::try_parse_from(["triad", "verify", "--claim", "REQ-auth-001"])
        .expect("verify cli should parse");
    let mut verify_stdout = Vec::new();

    let verify_exit = execute_cli_from_dir(verify_cli, &mut verify_stdout, &repo_root)
        .expect("verify should succeed");
    let verify_output = String::from_utf8(verify_stdout).expect("stdout should be utf8");

    assert_eq!(verify_exit as u8, 0);
    assert_eq!(
        verify_output,
        "REQ-auth-001  confirmed\nEvidence: EVID-000001\n"
    );
}
