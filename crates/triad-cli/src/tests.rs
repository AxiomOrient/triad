use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::anyhow;
use clap::error::ErrorKind;

use super::*;
use crate::agent_output::{AGENT_SCHEMA_VERSION, write_agent_envelope};
use crate::cli::{
    AcceptArgs, AgentArgs, AgentClaimArgs, AgentClaimCommand, AgentCommand, AgentDriftArgs,
    AgentDriftCommand, AgentPatchArgs, AgentPatchCommand, AgentRunArgs, AgentStatusArgs,
    AgentVerifyArgs, Effort, StatusArgs, VerifyArgs, WorkArgs,
};
use crate::exit_codes::{
    exit_code_for_claim_summaries, exit_code_for_drift, exit_code_for_next, exit_code_for_verify,
};
use triad_core::{
    ApplyPatchReport, Claim, ClaimBundle, ClaimSummary, DriftReport, DriftStatus, EvidenceId,
    NextAction, NextClaim, ProposePatchReport, RunClaimReport, RunClaimRequest, StatusReport,
    StatusSummary, Verdict, VerifyLayer, VerifyReport,
};

struct FakeRuntime {
    calls: RefCell<Vec<String>>,
    verify_requests: RefCell<Vec<VerifyRequest>>,
    run_requests: RefCell<Vec<RunClaimRequest>>,
    status_claims: RefCell<Vec<Option<ClaimId>>>,
    status_report: RefCell<Option<StatusReport>>,
    applied_patches: RefCell<Vec<PatchId>>,
    proposed_claims: RefCell<Vec<ClaimId>>,
    drift_claims: RefCell<Vec<ClaimId>>,
    get_claims: RefCell<Vec<ClaimId>>,
    default_verify_requests: RefCell<Vec<(ClaimId, bool, bool)>>,
    claim_diagnostics: RefCell<Vec<String>>,
}

impl FakeRuntime {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            verify_requests: RefCell::new(Vec::new()),
            run_requests: RefCell::new(Vec::new()),
            status_claims: RefCell::new(Vec::new()),
            status_report: RefCell::new(None),
            applied_patches: RefCell::new(Vec::new()),
            proposed_claims: RefCell::new(Vec::new()),
            drift_claims: RefCell::new(Vec::new()),
            get_claims: RefCell::new(Vec::new()),
            default_verify_requests: RefCell::new(Vec::new()),
            claim_diagnostics: RefCell::new(Vec::new()),
        }
    }

    fn set_status_report(&self, report: StatusReport) {
        *self.status_report.borrow_mut() = Some(report);
    }
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("triad-cli-{label}-{}-{unique}", process::id()));
        fs::create_dir_all(&path).expect("test directory should be created");
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

fn parse_json_line(line: &str) -> serde_json::Value {
    serde_json::from_str(line).expect("agent envelope should parse")
}

fn run_smoke_cli(runtime: &FakeRuntime, argv: &[&str]) -> (CliExit, String, String) {
    let cli = Cli::try_parse_from(argv).expect("argv should parse");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = match cli.command {
        Command::Init(_) => panic!("smoke helper does not cover init"),
        command => dispatch_command(runtime, command, &mut stdout),
    };
    let exit = finalize_cli_result(result, &mut stderr);

    (
        exit,
        String::from_utf8(stdout).expect("stdout should be utf8"),
        String::from_utf8(stderr).expect("stderr should be utf8"),
    )
}

fn assert_common_agent_envelope(parsed: &serde_json::Value, command: &str) {
    assert_eq!(parsed["schema_version"], AGENT_SCHEMA_VERSION);
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["command"], command);
    assert!(parsed.get("data").is_some());
    assert_eq!(parsed["diagnostics"], serde_json::json!([]));
}

fn load_schema(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas")
        .join(name);
    serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read schema {}: {err}", path.display())),
    )
    .unwrap_or_else(|err| panic!("failed to parse schema {}: {err}", path.display()))
}

fn assert_output_matches_schema(parsed: &serde_json::Value, schema_name: &str, command: &str) {
    let schema = load_schema(schema_name);
    assert_eq!(
        schema["type"],
        serde_json::Value::String("object".to_string()),
        "agent schemas must declare a top-level object type for runtime compatibility"
    );
    for subschema in schema["allOf"]
        .as_array()
        .expect("schema should declare allOf")
    {
        if let Some(reference) = subschema.get("$ref").and_then(serde_json::Value::as_str) {
            assert_eq!(reference, "./envelope.schema.json");
            assert_json_matches_schema(parsed, &load_schema("envelope.schema.json"));
        } else {
            assert_json_matches_schema(parsed, subschema);
        }
    }

    assert_eq!(
        schema["allOf"][1]["properties"]["command"]["const"], command,
        "schema command constant should match expected command"
    );
}

fn assert_json_matches_schema(value: &serde_json::Value, schema: &serde_json::Value) {
    if let Some(types) = schema.get("type") {
        assert!(
            value_matches_declared_type(value, types),
            "value {value:?} does not satisfy schema type {types:?}"
        );
    }

    if let Some(constraint) = schema.get("const") {
        assert_eq!(
            value, constraint,
            "value {value:?} does not satisfy schema const {constraint:?}"
        );
    }

    if let Some(enum_values) = schema.get("enum").and_then(serde_json::Value::as_array) {
        assert!(
            enum_values.iter().any(|candidate| candidate == value),
            "value {value:?} is not allowed by enum {enum_values:?}"
        );
    }

    if let Some(pattern) = schema.get("pattern").and_then(serde_json::Value::as_str) {
        let rendered = value
            .as_str()
            .expect("pattern constraints apply only to strings in current schemas");
        assert!(
            simple_pattern_match(rendered, pattern),
            "value `{rendered}` does not match pattern `{pattern}`"
        );
    }

    if let Some(minimum) = schema.get("minimum").and_then(serde_json::Value::as_i64) {
        let actual = value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .expect("minimum constraints apply only to integers in current schemas");
        assert!(
            actual >= minimum,
            "value {actual} is smaller than minimum {minimum}"
        );
    }

    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        let object = value
            .as_object()
            .expect("required constraints apply only to objects in current schemas");
        for field in required {
            let field = field
                .as_str()
                .expect("required field name should be a string");
            assert!(
                object.contains_key(field),
                "object {object:?} is missing required field `{field}`"
            );
        }
    }

    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        let object = value
            .as_object()
            .expect("properties constraints apply only to objects in current schemas");

        if schema.get("additionalProperties") == Some(&serde_json::Value::Bool(false)) {
            for field in object.keys() {
                assert!(
                    properties.contains_key(field),
                    "object field `{field}` is not declared by schema"
                );
            }
        }

        for (field, subschema) in properties {
            if let Some(child) = object.get(field) {
                assert_json_matches_schema(child, subschema);
            }
        }
    }

    if let Some(items) = schema.get("items") {
        let array = value
            .as_array()
            .expect("items constraints apply only to arrays in current schemas");
        for item in array {
            assert_json_matches_schema(item, items);
        }
    }
}

fn value_matches_declared_type(
    value: &serde_json::Value,
    declared_type: &serde_json::Value,
) -> bool {
    match declared_type {
        serde_json::Value::String(kind) => value_matches_type_name(value, kind),
        serde_json::Value::Array(kinds) => kinds
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|kind| value_matches_type_name(value, kind)),
        _ => false,
    }
}

fn value_matches_type_name(value: &serde_json::Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => {
            value.as_i64().is_some()
                || value
                    .as_u64()
                    .and_then(|number| i64::try_from(number).ok())
                    .is_some()
        }
        "null" => value.is_null(),
        _ => false,
    }
}

fn simple_pattern_match(value: &str, pattern: &str) -> bool {
    match pattern {
        "^REQ-[a-z0-9-]+\\-\\d{3}$" => {
            let Some((prefix, digits)) = value.rsplit_once('-') else {
                return false;
            };
            prefix.starts_with("REQ-")
                && digits.len() == 3
                && digits.chars().all(|ch| ch.is_ascii_digit())
                && prefix["REQ-".len()..]
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        }
        "^RUN-\\d{6}$" => {
            value.starts_with("RUN-")
                && value.len() == "RUN-".len() + 6
                && value["RUN-".len()..].chars().all(|ch| ch.is_ascii_digit())
        }
        "^PATCH-\\d{6}$" => {
            value.starts_with("PATCH-")
                && value.len() == "PATCH-".len() + 6
                && value["PATCH-".len()..]
                    .chars()
                    .all(|ch| ch.is_ascii_digit())
        }
        "^EVID-\\d{6}$" => {
            value.starts_with("EVID-")
                && value.len() == "EVID-".len() + 6
                && value["EVID-".len()..].chars().all(|ch| ch.is_ascii_digit())
        }
        other => panic!("unsupported schema pattern in test validator: {other}"),
    }
}

impl CliRuntime for FakeRuntime {
    fn default_verify_request(
        &self,
        claim_id: ClaimId,
        with_probe: bool,
        full_workspace: bool,
    ) -> Result<VerifyRequest, TriadError> {
        self.calls
            .borrow_mut()
            .push("default_verify_request".to_string());
        self.default_verify_requests.borrow_mut().push((
            claim_id.clone(),
            with_probe,
            full_workspace,
        ));

        Ok(VerifyRequest {
            claim_id,
            layers: if with_probe {
                vec![
                    VerifyLayer::Unit,
                    VerifyLayer::Contract,
                    VerifyLayer::Integration,
                    VerifyLayer::Probe,
                ]
            } else {
                vec![
                    VerifyLayer::Unit,
                    VerifyLayer::Contract,
                    VerifyLayer::Integration,
                ]
            },
            full_workspace,
        })
    }

    fn claim_load_diagnostics(&self) -> Result<Vec<String>, TriadError> {
        Ok(self.claim_diagnostics.borrow().clone())
    }
}

impl TriadApi for FakeRuntime {
    fn ingest_spec(&self) -> Result<triad_core::IngestReport, TriadError> {
        self.calls.borrow_mut().push("ingest_spec".to_string());
        Ok(triad_core::IngestReport { claim_count: 2 })
    }

    fn list_claims(&self) -> Result<Vec<ClaimSummary>, TriadError> {
        self.calls.borrow_mut().push("list_claims".to_string());
        Ok(vec![ClaimSummary {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            title: "Login".to_string(),
            status: DriftStatus::NeedsTest,
            revision: 1,
            pending_patch_id: None,
        }])
    }

    fn get_claim(&self, id: &ClaimId) -> Result<ClaimBundle, TriadError> {
        self.calls.borrow_mut().push("get_claim".to_string());
        self.get_claims.borrow_mut().push(id.clone());
        Ok(ClaimBundle {
            claim: Claim {
                id: id.clone(),
                title: "Login".to_string(),
                statement: "User logs in.".to_string(),
                examples: vec!["valid -> success".to_string()],
                invariants: vec!["no plaintext".to_string()],
                notes: None,
                revision: 1,
            },
            drift: DriftReport {
                claim_id: id.clone(),
                status: DriftStatus::NeedsTest,
                reasons: vec!["needs verify".to_string()],
                fresh_evidence_ids: vec![],
                pending_patch_id: None,
            },
        })
    }

    fn next_claim(&self) -> Result<NextClaim, TriadError> {
        self.calls.borrow_mut().push("next_claim".to_string());
        Ok(NextClaim {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            status: DriftStatus::NeedsTest,
            reason: "needs verify".to_string(),
            next_action: NextAction::Verify,
        })
    }

    fn detect_drift(&self, id: &ClaimId) -> Result<DriftReport, TriadError> {
        self.calls.borrow_mut().push("detect_drift".to_string());
        self.drift_claims.borrow_mut().push(id.clone());
        Ok(DriftReport {
            claim_id: id.clone(),
            status: DriftStatus::Contradicted,
            reasons: vec!["latest verify failed".to_string()],
            fresh_evidence_ids: vec![],
            pending_patch_id: None,
        })
    }

    fn run_claim(&self, req: RunClaimRequest) -> Result<RunClaimReport, TriadError> {
        self.calls.borrow_mut().push("run_claim".to_string());
        self.run_requests.borrow_mut().push(req.clone());
        Ok(RunClaimReport {
            run_id: triad_core::RunId::new("RUN-000001").expect("run id should parse"),
            claim_id: req.claim_id,
            summary: "ran work".to_string(),
            changed_paths: vec!["src/auth.rs".to_string()],
            suggested_test_selectors: vec!["auth::unit".to_string()],
            blocked_actions: vec![],
            needs_patch: false,
        })
    }

    fn verify_claim(&self, req: VerifyRequest) -> Result<VerifyReport, TriadError> {
        self.calls.borrow_mut().push("verify_claim".to_string());
        self.verify_requests.borrow_mut().push(req.clone());
        Ok(VerifyReport {
            claim_id: req.claim_id,
            verdict: Verdict::Pass,
            layers: req.layers,
            full_workspace: req.full_workspace,
            evidence_ids: vec![EvidenceId::new("EVID-000001").expect("evidence id should parse")],
            status_after_verify: DriftStatus::Healthy,
            pending_patch_id: None,
        })
    }

    fn propose_patch(&self, id: &ClaimId) -> Result<ProposePatchReport, TriadError> {
        self.calls.borrow_mut().push("propose_patch".to_string());
        self.proposed_claims.borrow_mut().push(id.clone());
        Ok(ProposePatchReport {
            patch_id: PatchId::new("PATCH-000001").expect("patch id should parse"),
            claim_id: id.clone(),
            based_on_evidence: vec![
                EvidenceId::new("EVID-000001").expect("evidence id should parse"),
            ],
            path: "spec/claims/REQ-auth-001.md".to_string(),
            reason: "behavior changed".to_string(),
        })
    }

    fn apply_patch(&self, id: &PatchId) -> Result<ApplyPatchReport, TriadError> {
        self.calls.borrow_mut().push("apply_patch".to_string());
        self.applied_patches.borrow_mut().push(id.clone());
        Ok(ApplyPatchReport {
            patch_id: id.clone(),
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            applied: true,
            new_revision: 2,
            followup_action: NextAction::Verify,
        })
    }

    fn status(&self, claim: Option<&ClaimId>) -> Result<StatusReport, TriadError> {
        self.calls.borrow_mut().push("status".to_string());
        self.status_claims.borrow_mut().push(claim.cloned());
        if let Some(report) = self.status_report.borrow().clone() {
            Ok(report)
        } else {
            Ok(StatusReport {
                summary: StatusSummary {
                    healthy: 0,
                    needs_code: 0,
                    needs_test: 1,
                    needs_spec: 0,
                    contradicted: 0,
                    blocked: 0,
                },
                claims: vec![ClaimSummary {
                    claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                    title: "Login".to_string(),
                    status: DriftStatus::NeedsTest,
                    revision: 1,
                    pending_patch_id: None,
                }],
            })
        }
    }
}

#[test]
fn cli_wiring_human_work_and_verify_resolve_runtime_calls() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Work(WorkArgs {
            claim_id: None,
            dry_run: true,
            model: Some("gpt-5-codex".to_string()),
            effort: Some(Effort::High),
        }),
        &mut stdout,
    )
    .expect("work should dispatch");
    dispatch_command(
        &runtime,
        Command::Verify(VerifyArgs {
            claim_id: None,
            with_probe: true,
            full_workspace: true,
        }),
        &mut stdout,
    )
    .expect("verify should dispatch");

    assert_eq!(
        runtime.calls.borrow().as_slice(),
        &[
            "next_claim",
            "run_claim",
            "next_claim",
            "default_verify_request",
            "verify_claim",
        ]
    );
    assert_eq!(runtime.run_requests.borrow().len(), 1);
    assert_eq!(
        runtime.run_requests.borrow()[0].effort,
        Some(triad_core::ReasoningLevel::High)
    );
    assert_eq!(runtime.verify_requests.borrow().len(), 1);
    assert_eq!(
        runtime.verify_requests.borrow()[0].layers,
        vec![
            VerifyLayer::Unit,
            VerifyLayer::Contract,
            VerifyLayer::Integration,
            VerifyLayer::Probe,
        ]
    );
    assert!(
        String::from_utf8(stdout)
            .expect("stdout should be utf8")
            .contains("Next: triad status --claim REQ-auth-001")
    );
}

#[test]
fn smoke_help_reports_top_level_usage() {
    let error = Cli::try_parse_from(["triad", "--help"]).expect_err("help should exit early");

    assert_eq!(error.kind(), ErrorKind::DisplayHelp);

    let rendered = error.to_string();
    assert!(rendered.contains("Claim/evidence ratchet CLI"));
    assert!(rendered.contains("next"));
    assert!(rendered.contains("status"));
    assert!(rendered.contains("agent"));
}

#[test]
fn smoke_accept_help_shows_latest_and_omits_removed_yes_flag() {
    let error = Cli::try_parse_from(["triad", "accept", "--help"]).expect_err("help should exit");

    assert_eq!(error.kind(), ErrorKind::DisplayHelp);

    let rendered = error.to_string();
    assert!(rendered.contains("--latest"));
    assert!(!rendered.contains("--yes"));
}

#[test]
fn cli_wiring_accept_rejects_removed_yes_flag() {
    let error = Cli::try_parse_from(["triad", "accept", "PATCH-000001", "--yes"])
        .expect_err("--yes should be rejected");

    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn cli_wiring_accept_resolves_latest_pending_patch_deterministically() {
    let runtime = FakeRuntime::new();
    runtime.set_status_report(StatusReport {
        summary: StatusSummary {
            healthy: 0,
            needs_code: 0,
            needs_test: 0,
            needs_spec: 2,
            contradicted: 0,
            blocked: 0,
        },
        claims: vec![
            ClaimSummary {
                claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
                title: "Login".to_string(),
                status: DriftStatus::NeedsSpec,
                revision: 1,
                pending_patch_id: Some(
                    PatchId::new("PATCH-000001").expect("patch id should parse"),
                ),
            },
            ClaimSummary {
                claim_id: ClaimId::new("REQ-auth-002").expect("claim id should parse"),
                title: "Logout".to_string(),
                status: DriftStatus::NeedsSpec,
                revision: 1,
                pending_patch_id: Some(
                    PatchId::new("PATCH-000003").expect("patch id should parse"),
                ),
            },
        ],
    });
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Accept(AcceptArgs {
            patch_id: None,
            latest: true,
        }),
        &mut stdout,
    )
    .expect("accept --latest should dispatch");

    assert_eq!(
        runtime.calls.borrow().as_slice(),
        &["status", "apply_patch"]
    );
    assert_eq!(
        runtime.applied_patches.borrow().as_slice(),
        &[PatchId::new("PATCH-000003").expect("patch id should parse")]
    );
}

#[test]
fn cli_wiring_accept_latest_errors_when_no_pending_patch_exists() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    let error = dispatch_command(
        &runtime,
        Command::Accept(AcceptArgs {
            patch_id: None,
            latest: true,
        }),
        &mut stdout,
    )
    .expect_err("accept --latest should fail without a pending patch");

    assert_eq!(error.to_string(), "no pending patch exists for --latest");
    assert_eq!(runtime.calls.borrow().as_slice(), &["status"]);
    assert!(runtime.applied_patches.borrow().is_empty());
}

#[test]
fn smoke_next_command_parses_and_runs_from_cli_entrypoint() {
    let runtime = FakeRuntime::new();

    let (exit, stdout, stderr) = run_smoke_cli(&runtime, &["triad", "next"]);

    assert_eq!(exit, CliExit::DriftDetected);
    assert!(stderr.is_empty());
    assert!(stdout.contains("REQ-auth-001  needs-test"));
    assert!(stdout.contains("Next: triad verify REQ-auth-001"));
    assert_eq!(runtime.calls.borrow().as_slice(), &["next_claim"]);
}

#[test]
fn smoke_agent_status_command_parses_and_runs_from_cli_entrypoint() {
    let runtime = FakeRuntime::new();

    let (exit, stdout, stderr) = run_smoke_cli(
        &runtime,
        &["triad", "agent", "status", "--claim", "REQ-auth-001"],
    );

    assert_eq!(exit, CliExit::DriftDetected);
    assert!(stderr.is_empty());
    assert_eq!(
        runtime.status_claims.borrow().as_slice(),
        &[Some(
            ClaimId::new("REQ-auth-001").expect("claim id should parse")
        )]
    );

    let parsed = parse_json_line(stdout.trim_end());
    assert_common_agent_envelope(&parsed, "status");
    assert_eq!(parsed["data"]["claims"][0]["claim_id"], "REQ-auth-001");
}

#[test]
fn smoke_init_command_bootstraps_empty_working_dir() {
    let temp = TestDir::new("smoke-init");
    let cli = Cli::try_parse_from(["triad", "init"]).expect("argv should parse");
    let mut stdout = Vec::new();

    let exit = execute_cli_from_dir(cli, &mut stdout, temp.path()).expect("init should run");

    assert_eq!(exit, CliExit::Success);
    assert!(stdout.is_empty());
    assert!(temp.path().join("triad.toml").is_file());
    assert!(temp.path().join("AGENTS.md").is_file());
    assert!(temp.path().join(".gitignore").is_file());
    assert!(temp.path().join("docs").is_dir());
    assert!(temp.path().join("schemas").is_dir());
    assert!(temp.path().join("spec/claims").is_dir());
    assert!(temp.path().join(".triad/evidence.ndjson").is_file());
    assert!(temp.path().join(".triad/patches").is_dir());
    assert!(temp.path().join(".triad/runs").is_dir());
}

#[test]
fn work_output_prints_summary_blockers_and_follow_up() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Work(WorkArgs {
            claim_id: Some("REQ-auth-001".to_string()),
            dry_run: true,
            model: None,
            effort: None,
        }),
        &mut stdout,
    )
    .expect("work should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "REQ-auth-001  work");
    assert!(output.contains("Summary: ran work"));
    assert!(output.contains("Changed paths: src/auth.rs"));
    assert!(output.contains("Suggested tests: auth::unit"));
    assert!(output.contains("Blockers: none"));
    assert_eq!(
        lines.last().expect("work output should have last line"),
        &"Next: triad verify REQ-auth-001"
    );
    assert!(!output.starts_with('{'));
}

#[test]
fn verify_output_prints_summary_blockers_and_follow_up() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Verify(VerifyArgs {
            claim_id: Some("REQ-auth-001".to_string()),
            with_probe: false,
            full_workspace: true,
        }),
        &mut stdout,
    )
    .expect("verify should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "REQ-auth-001  verify  pass");
    assert!(output.contains("Summary: status -> healthy"));
    assert!(output.contains("Layers: unit, contract, integration"));
    assert!(output.contains("Scope: full-workspace"));
    assert!(output.contains("Evidence: EVID-000001"));
    assert!(output.contains("Blockers: none"));
    assert_eq!(
        lines.last().expect("verify output should have last line"),
        &"Next: triad status --claim REQ-auth-001"
    );
    assert!(!output.starts_with('{'));
}

#[test]
fn accept_output_prints_summary_blockers_and_follow_up() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Accept(AcceptArgs {
            patch_id: Some("PATCH-000001".to_string()),
            latest: false,
        }),
        &mut stdout,
    )
    .expect("accept should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "PATCH-000001  accept");
    assert!(output.contains("Summary: applied for REQ-auth-001"));
    assert!(output.contains("New revision: 2"));
    assert!(output.contains("Blockers: none"));
    assert_eq!(
        lines.last().expect("accept output should have last line"),
        &"Next: triad verify REQ-auth-001"
    );
    assert!(!output.starts_with('{'));
}

#[test]
fn next_output_prints_recommended_command_last() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(&runtime, Command::Next, &mut stdout).expect("next should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "REQ-auth-001  needs-test");
    assert!(output.contains("Reason: needs verify"));
    assert!(output.contains("Suggested: triad verify REQ-auth-001"));
    assert_eq!(
        lines.last().expect("next output should have last line"),
        &"Next: triad verify REQ-auth-001"
    );
    assert!(!output.starts_with('{'));
}

#[test]
fn malformed_claim_next_output_includes_problem_diagnostics() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();
    runtime.claim_diagnostics.borrow_mut().push(
        "malformed claim REQ-auth-099: parse error: missing required section Invariants in spec/claims/REQ-auth-099.md".to_string(),
    );

    dispatch_command(&runtime, Command::Next, &mut stdout).expect("next should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");

    assert!(output.contains("Problems:"));
    assert!(output.contains(
        "malformed claim REQ-auth-099: parse error: missing required section Invariants"
    ));
    assert!(output.contains("Next: triad verify REQ-auth-001"));
}

#[test]
fn status_output_prints_summary_and_recommended_command_last() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Status(StatusArgs {
            claim: None,
            verbose: false,
        }),
        &mut stdout,
    )
    .expect("status should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines[0], "Claims: 1");
    assert!(lines[1].contains("Needs-test: 1"));
    assert!(output.contains("REQ-auth-001  needs-test  Login"));
    assert!(output.contains("Suggested: triad verify REQ-auth-001"));
    assert_eq!(
        lines.last().expect("status output should have last line"),
        &"Next: triad verify REQ-auth-001"
    );
    assert!(!output.starts_with('{'));
}

#[test]
fn malformed_claim_status_output_includes_problem_diagnostics() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();
    runtime.claim_diagnostics.borrow_mut().push(
        "malformed claim REQ-auth-099: parse error: missing required section Invariants in spec/claims/REQ-auth-099.md".to_string(),
    );

    dispatch_command(
        &runtime,
        Command::Status(StatusArgs {
            claim: None,
            verbose: false,
        }),
        &mut stdout,
    )
    .expect("status should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");

    assert!(output.contains("Problems:"));
    assert!(output.contains(
        "malformed claim REQ-auth-099: parse error: missing required section Invariants"
    ));
    assert!(output.contains("Next: triad verify REQ-auth-001"));
}

#[test]
fn human_formatting_keeps_agent_output_on_compact_envelope_path() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Next,
            }),
        }),
        &mut stdout,
    )
    .expect("agent next should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(&output);

    assert_common_agent_envelope(&parsed, "claim.next");
    assert!(output.starts_with('{'));
    assert!(!output.contains("\n  \""));
}

#[test]
fn agent_envelope_writer_emits_common_fields_and_compact_json() {
    let mut stdout = Vec::new();

    write_agent_envelope(
        &mut stdout,
        "claim.next",
        &serde_json::json!({
            "claim_id": "REQ-auth-001",
            "status": "needs-test"
        }),
    )
    .expect("agent envelope should write");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "claim.next");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert!(output.ends_with('\n'));
    assert!(!output.contains("\n  \""));
}

#[test]
fn agent_envelope_is_shared_across_agent_commands() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::List,
            }),
        }),
        &mut stdout,
    )
    .expect("claim list should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Run(AgentRunArgs {
                claim: "REQ-auth-001".to_string(),
            }),
        }),
        &mut stdout,
    )
    .expect("run should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Status(AgentStatusArgs {
                claim: Some("REQ-auth-001".to_string()),
            }),
        }),
        &mut stdout,
    )
    .expect("status should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 3);

    let claim_list = parse_json_line(lines[0]);
    assert_common_agent_envelope(&claim_list, "claim.list");
    assert!(claim_list["data"].is_object());
    assert!(claim_list["data"]["claims"].is_array());

    let run = parse_json_line(lines[1]);
    assert_common_agent_envelope(&run, "run");
    assert_eq!(run["data"]["run_id"], "RUN-000001");

    let status = parse_json_line(lines[2]);
    assert_common_agent_envelope(&status, "status");
    assert_eq!(status["data"]["summary"]["needs_test"], 1);
}

#[test]
fn cli_wiring_agent_commands_emit_runtime_backed_json() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::List,
            }),
        }),
        &mut stdout,
    )
    .expect("claim list should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Drift(AgentDriftArgs {
                command: AgentDriftCommand::Detect {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("drift detect should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Apply {
                    patch: "PATCH-000001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("patch apply should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);

    let first = parse_json_line(lines[0]);
    assert_common_agent_envelope(&first, "claim.list");
    assert_eq!(first["data"]["claims"][0]["claim_id"], "REQ-auth-001");

    let second = parse_json_line(lines[1]);
    assert_common_agent_envelope(&second, "drift.detect");
    assert_eq!(second["data"]["status"], "contradicted");

    let third = parse_json_line(lines[2]);
    assert_common_agent_envelope(&third, "patch.apply");
    assert_eq!(third["data"]["patch_id"], "PATCH-000001");
}

#[test]
fn cli_wiring_covers_remaining_runtime_routes() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(&runtime, Command::Next, &mut stdout).expect("next should dispatch");
    dispatch_command(
        &runtime,
        Command::Status(StatusArgs {
            claim: Some("REQ-auth-001".to_string()),
            verbose: false,
        }),
        &mut stdout,
    )
    .expect("status should dispatch");
    dispatch_command(
        &runtime,
        Command::Accept(AcceptArgs {
            patch_id: Some("PATCH-000001".to_string()),
            latest: false,
        }),
        &mut stdout,
    )
    .expect("accept should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Get {
                    claim_id: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("claim get should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Next,
            }),
        }),
        &mut stdout,
    )
    .expect("claim next should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Drift(AgentDriftArgs {
                command: AgentDriftCommand::Detect {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("drift detect should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Run(AgentRunArgs {
                claim: "REQ-auth-001".to_string(),
            }),
        }),
        &mut stdout,
    )
    .expect("run should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Verify(AgentVerifyArgs {
                claim: "REQ-auth-001".to_string(),
                with_probe: false,
                full_workspace: false,
            }),
        }),
        &mut stdout,
    )
    .expect("agent verify should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Propose {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("patch propose should dispatch");
    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Status(AgentStatusArgs {
                claim: Some("REQ-auth-001".to_string()),
            }),
        }),
        &mut stdout,
    )
    .expect("agent status should dispatch");

    assert_eq!(
        runtime.applied_patches.borrow().as_slice(),
        &[PatchId::new("PATCH-000001").expect("patch id should parse")]
    );
    assert_eq!(
        runtime.proposed_claims.borrow().as_slice(),
        &[ClaimId::new("REQ-auth-001").expect("claim id should parse")]
    );
    assert_eq!(
        runtime.drift_claims.borrow().as_slice(),
        &[ClaimId::new("REQ-auth-001").expect("claim id should parse")]
    );
    assert_eq!(
        runtime.get_claims.borrow().as_slice(),
        &[ClaimId::new("REQ-auth-001").expect("claim id should parse")]
    );
    assert_eq!(
        runtime.status_claims.borrow().as_slice(),
        &[
            Some(ClaimId::new("REQ-auth-001").expect("claim id should parse")),
            Some(ClaimId::new("REQ-auth-001").expect("claim id should parse")),
        ]
    );
    assert_eq!(runtime.verify_requests.borrow().len(), 1);
    assert_eq!(runtime.run_requests.borrow().len(), 1);
    assert!(
        String::from_utf8(stdout)
            .expect("stdout should be utf8")
            .contains("REQ-auth-001")
    );
}

#[test]
fn agent_claims_list_wraps_claims_under_data_claims() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::List,
            }),
        }),
        &mut stdout,
    )
    .expect("claim list should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "claim.list");
    assert_eq!(parsed["data"]["claims"][0]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["claims"][0]["title"], "Login");
    assert_eq!(parsed["data"]["claims"][0]["status"], "needs-test");
    assert_eq!(parsed["data"]["claims"][0]["revision"], 1);
}

#[test]
fn agent_claims_get_flattens_claim_bundle_to_claim_fields() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Get {
                    claim_id: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("claim get should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "claim.get");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["title"], "Login");
    assert_eq!(parsed["data"]["statement"], "User logs in.");
    assert_eq!(
        parsed["data"]["examples"],
        serde_json::json!(["valid -> success"])
    );
    assert_eq!(
        parsed["data"]["invariants"],
        serde_json::json!(["no plaintext"])
    );
    assert!(parsed["data"].get("drift").is_none());
}

#[test]
fn agent_claims_next_emits_machine_readable_next_claim() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Next,
            }),
        }),
        &mut stdout,
    )
    .expect("claim next should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "claim.next");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["status"], "needs-test");
    assert_eq!(parsed["data"]["reason"], "needs verify");
    assert_eq!(parsed["data"]["next_action"], "verify");
}

#[test]
fn agent_drift_detect_emits_machine_readable_drift_payload() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Drift(AgentDriftArgs {
                command: AgentDriftCommand::Detect {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("drift detect should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "drift.detect");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["status"], "contradicted");
    assert_eq!(
        parsed["data"]["reasons"],
        serde_json::json!(["latest verify failed"])
    );
}

#[test]
fn agent_status_emits_summary_and_claims_payload() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Status(AgentStatusArgs {
                claim: Some("REQ-auth-001".to_string()),
            }),
        }),
        &mut stdout,
    )
    .expect("status should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_common_agent_envelope(&parsed, "status");
    assert_eq!(parsed["data"]["summary"]["needs_test"], 1);
    assert_eq!(parsed["data"]["claims"][0]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["claims"][0]["status"], "needs-test");
}

#[test]
fn agent_run_emits_machine_readable_run_payload_matching_schema() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Run(AgentRunArgs {
                claim: "REQ-auth-001".to_string(),
            }),
        }),
        &mut stdout,
    )
    .expect("agent run should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_output_matches_schema(&parsed, "agent.run.schema.json", "run");
    assert_eq!(parsed["data"]["run_id"], "RUN-000001");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["summary"], "ran work");
    assert_eq!(
        parsed["data"]["changed_paths"],
        serde_json::json!(["src/auth.rs"])
    );
    assert_eq!(
        parsed["data"]["suggested_test_selectors"],
        serde_json::json!(["auth::unit"])
    );
    assert_eq!(parsed["data"]["blocked_actions"], serde_json::json!([]));
    assert_eq!(parsed["data"]["needs_patch"], false);
}

#[test]
fn agent_verify_emits_machine_readable_verify_payload_matching_schema() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Verify(AgentVerifyArgs {
                claim: "REQ-auth-001".to_string(),
                with_probe: true,
                full_workspace: true,
            }),
        }),
        &mut stdout,
    )
    .expect("agent verify should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_output_matches_schema(&parsed, "agent.verify.schema.json", "verify");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["verdict"], "pass");
    assert_eq!(
        parsed["data"]["layers"],
        serde_json::json!(["unit", "contract", "integration", "probe"])
    );
    assert_eq!(parsed["data"]["full_workspace"], true);
    assert_eq!(
        parsed["data"]["evidence_ids"],
        serde_json::json!(["EVID-000001"])
    );
    assert_eq!(parsed["data"]["status_after_verify"], "healthy");
    assert_eq!(parsed["data"]["pending_patch_id"], serde_json::Value::Null);
    assert_eq!(
        runtime.default_verify_requests.borrow().as_slice(),
        &[(
            ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            true,
            true,
        )]
    );
}

#[test]
fn agent_patch_propose_emits_machine_readable_payload_matching_schema() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Propose {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("agent patch propose should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_output_matches_schema(&parsed, "agent.patch.propose.schema.json", "patch.propose");
    assert_eq!(parsed["data"]["patch_id"], "PATCH-000001");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(
        parsed["data"]["based_on_evidence"],
        serde_json::json!(["EVID-000001"])
    );
    assert_eq!(parsed["data"]["path"], "spec/claims/REQ-auth-001.md");
    assert_eq!(parsed["data"]["reason"], "behavior changed");
    assert_eq!(
        runtime.proposed_claims.borrow().as_slice(),
        &[ClaimId::new("REQ-auth-001").expect("claim id should parse")]
    );
}

#[test]
fn agent_patch_apply_emits_machine_readable_payload_matching_schema() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Apply {
                    patch: "PATCH-000001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("agent patch apply should dispatch");

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_output_matches_schema(&parsed, "agent.patch.apply.schema.json", "patch.apply");
    assert_eq!(parsed["data"]["patch_id"], "PATCH-000001");
    assert_eq!(parsed["data"]["claim_id"], "REQ-auth-001");
    assert_eq!(parsed["data"]["applied"], true);
    assert_eq!(parsed["data"]["new_revision"], 2);
    assert_eq!(parsed["data"]["followup_action"], "verify");
    assert_eq!(
        runtime.applied_patches.borrow().as_slice(),
        &[PatchId::new("PATCH-000001").expect("patch id should parse")]
    );
}

#[test]
fn exit_codes_map_status_results_to_documented_numbers() {
    let healthy_claim = ClaimSummary {
        claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
        title: "Login".to_string(),
        status: DriftStatus::Healthy,
        revision: 1,
        pending_patch_id: None,
    };
    let needs_test_claim = ClaimSummary {
        claim_id: ClaimId::new("REQ-auth-002").expect("claim id should parse"),
        title: "Logout".to_string(),
        status: DriftStatus::NeedsTest,
        revision: 1,
        pending_patch_id: None,
    };
    let needs_spec_claim = ClaimSummary {
        claim_id: ClaimId::new("REQ-auth-003").expect("claim id should parse"),
        title: "Reset".to_string(),
        status: DriftStatus::NeedsSpec,
        revision: 1,
        pending_patch_id: Some(PatchId::new("PATCH-000001").expect("patch id should parse")),
    };

    assert_eq!(
        exit_code_for_claim_summaries(&[healthy_claim]),
        CliExit::Success
    );
    assert_eq!(
        exit_code_for_claim_summaries(&[needs_test_claim]),
        CliExit::DriftDetected
    );
    assert_eq!(
        exit_code_for_claim_summaries(&[needs_spec_claim]),
        CliExit::PatchApprovalRequired
    );
    assert_eq!(
        exit_code_for_next(&NextClaim {
            claim_id: ClaimId::new("REQ-auth-003").expect("claim id should parse"),
            status: DriftStatus::NeedsSpec,
            reason: "pending patch exists".to_string(),
            next_action: NextAction::Accept,
        }),
        CliExit::PatchApprovalRequired
    );
    assert_eq!(
        exit_code_for_verify(&VerifyReport {
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            verdict: Verdict::Fail,
            layers: vec![VerifyLayer::Unit],
            full_workspace: false,
            evidence_ids: vec![],
            status_after_verify: DriftStatus::Contradicted,
            pending_patch_id: None,
        }),
        CliExit::VerificationFailed
    );
}

#[test]
fn exit_codes_map_error_kinds_to_documented_numbers() {
    assert_eq!(
        exit_code_for_error(&anyhow::Error::from(TriadError::Parse(
            "bad claim id".to_string(),
        ))),
        CliExit::InvalidInput
    );
    assert_eq!(
        exit_code_for_error(&anyhow::Error::from(TriadError::RuntimeBlocked(
            "git push forbidden".to_string(),
        ))),
        CliExit::InvalidInput
    );
    assert_eq!(
        exit_code_for_error(&anyhow::Error::from(TriadError::VerificationFailed(
            "tests failed".to_string(),
        ))),
        CliExit::VerificationFailed
    );
    assert_eq!(
        exit_code_for_error(&anyhow::Error::from(TriadError::Io(
            "disk unavailable".to_string(),
        ))),
        CliExit::InternalError
    );
    assert_eq!(
        exit_code_for_error(&anyhow!("opaque failure")),
        CliExit::InternalError
    );
}

#[test]
fn blocked_exit_code_maps_drift_and_runtime_blocked_error_to_documented_numbers() {
    let blocked_claim = ClaimSummary {
        claim_id: ClaimId::new("REQ-auth-004").expect("claim id should parse"),
        title: "Blocked".to_string(),
        status: DriftStatus::Blocked,
        revision: 1,
        pending_patch_id: None,
    };

    assert_eq!(
        exit_code_for_claim_summaries(&[blocked_claim]),
        CliExit::DriftDetected
    );
    assert_eq!(
        exit_code_for_drift(&DriftReport {
            claim_id: ClaimId::new("REQ-auth-004").expect("claim id should parse"),
            status: DriftStatus::Blocked,
            reasons: vec!["approval required".to_string()],
            fresh_evidence_ids: vec![],
            pending_patch_id: None,
        }),
        CliExit::DriftDetected
    );
    assert_eq!(
        exit_code_for_error(&anyhow::Error::from(TriadError::RuntimeBlocked(
            "git push blocked by work guardrails".to_string(),
        ))),
        CliExit::InvalidInput
    );
}

#[test]
fn stdout_stderr_discipline_keeps_agent_json_on_stdout_only() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit = finalize_cli_result(
        dispatch_command(
            &runtime,
            Command::Agent(AgentArgs {
                command: AgentCommand::Claim(AgentClaimArgs {
                    command: AgentClaimCommand::Next,
                }),
            }),
            &mut stdout,
        ),
        &mut stderr,
    );

    let output = String::from_utf8(stdout).expect("stdout should be utf8");
    let parsed = parse_json_line(output.trim_end());

    assert_eq!(exit, CliExit::DriftDetected);
    assert_common_agent_envelope(&parsed, "claim.next");
    assert_eq!(
        String::from_utf8(stderr).expect("stderr should be utf8"),
        ""
    );
}

#[test]
fn stdout_stderr_discipline_routes_agent_errors_to_stderr_only() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let exit = finalize_cli_result(
        dispatch_command(
            &runtime,
            Command::Agent(AgentArgs {
                command: AgentCommand::Run(AgentRunArgs {
                    claim: "not-a-claim".to_string(),
                }),
            }),
            &mut stdout,
        ),
        &mut stderr,
    );

    assert_eq!(exit, CliExit::InvalidInput);
    assert_eq!(
        String::from_utf8(stdout).expect("stdout should be utf8"),
        ""
    );

    let diagnostics = String::from_utf8(stderr).expect("stderr should be utf8");
    assert!(diagnostics.contains("invalid claim id"));
    assert!(!diagnostics.trim_start().starts_with('{'));
}

#[test]
fn schema_contract_agent_claim_commands_match_schema_fixtures() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::List,
            }),
        }),
        &mut stdout,
    )
    .expect("claim list should dispatch");
    let list_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(list_output.trim_end()),
        "agent.claim.list.schema.json",
        "claim.list",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Get {
                    claim_id: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("claim get should dispatch");
    let get_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(get_output.trim_end()),
        "agent.claim.get.schema.json",
        "claim.get",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Claim(AgentClaimArgs {
                command: AgentClaimCommand::Next,
            }),
        }),
        &mut stdout,
    )
    .expect("claim next should dispatch");
    let next_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(next_output.trim_end()),
        "agent.claim.next.schema.json",
        "claim.next",
    );
}

#[test]
fn schema_contract_agent_drift_and_status_commands_match_schema_fixtures() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Drift(AgentDriftArgs {
                command: AgentDriftCommand::Detect {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("drift detect should dispatch");
    let drift_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(drift_output.trim_end()),
        "agent.drift.detect.schema.json",
        "drift.detect",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Status(AgentStatusArgs {
                claim: Some("REQ-auth-001".to_string()),
            }),
        }),
        &mut stdout,
    )
    .expect("status should dispatch");
    let status_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(status_output.trim_end()),
        "agent.status.schema.json",
        "status",
    );
}

#[test]
fn schema_contract_agent_run_verify_and_patch_commands_match_schema_fixtures() {
    let runtime = FakeRuntime::new();
    let mut stdout = Vec::new();

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Run(AgentRunArgs {
                claim: "REQ-auth-001".to_string(),
            }),
        }),
        &mut stdout,
    )
    .expect("run should dispatch");
    let run_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(run_output.trim_end()),
        "agent.run.schema.json",
        "run",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Verify(AgentVerifyArgs {
                claim: "REQ-auth-001".to_string(),
                with_probe: true,
                full_workspace: true,
            }),
        }),
        &mut stdout,
    )
    .expect("verify should dispatch");
    let verify_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(verify_output.trim_end()),
        "agent.verify.schema.json",
        "verify",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Propose {
                    claim: "REQ-auth-001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("patch propose should dispatch");
    let patch_propose_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(patch_propose_output.trim_end()),
        "agent.patch.propose.schema.json",
        "patch.propose",
    );

    dispatch_command(
        &runtime,
        Command::Agent(AgentArgs {
            command: AgentCommand::Patch(AgentPatchArgs {
                command: AgentPatchCommand::Apply {
                    patch: "PATCH-000001".to_string(),
                },
            }),
        }),
        &mut stdout,
    )
    .expect("patch apply should dispatch");
    let patch_apply_output = String::from_utf8(std::mem::take(&mut stdout)).expect("stdout utf8");
    assert_output_matches_schema(
        &parse_json_line(patch_apply_output.trim_end()),
        "agent.patch.apply.schema.json",
        "patch.apply",
    );
}
