use std::io::Write;

use anyhow::{Result, anyhow};
use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{Claim, ClaimId, TriadError, verify_claim};
use triad_fs::{
    CanonicalTriadConfig, ClaimMarkdownAdapter, CommandCapture, EvidenceNdjsonStore,
    SnapshotAdapter,
};

use crate::cli::{Command, LintArgs, ReportArgs, VerifyArgs};
use crate::exit_codes::CliExit;
use crate::output::{
    InitOutput, LintClaimOutput, LintOutput, OutputMode, VerifyOutput, write_init, write_lint,
    write_report, write_verify,
};
use crate::parsing::parse_claim_id;

#[derive(Debug, Clone)]
struct LoadedClaim {
    claim: Claim,
    path: Utf8PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedVerifyCommand {
    command: String,
    locator: Option<String>,
    artifact_include: Vec<String>,
}

pub(crate) fn dispatch_command(
    config: &CanonicalTriadConfig,
    command: Command,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    match command {
        Command::Init(_) => unreachable!("init must be handled before runtime dispatch"),
        Command::Lint(args) => dispatch_lint(config, args, stdout),
        Command::Verify(args) => dispatch_verify(config, args, stdout),
        Command::Report(args) => dispatch_report(config, args, stdout),
    }
}

pub(crate) fn dispatch_init(repo_root: &Utf8Path, stdout: &mut impl Write) -> Result<CliExit> {
    let output = InitOutput {
        repo_root: repo_root.as_str().to_string(),
        config_path: repo_root.join("triad.toml").as_str().to_string(),
        evidence_file: repo_root
            .join(".triad/evidence.ndjson")
            .as_str()
            .to_string(),
    };
    write_init(stdout, OutputMode::Human, &output)?;
    Ok(CliExit::Success)
}

fn dispatch_lint(
    config: &CanonicalTriadConfig,
    args: LintArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let claims = load_claims(config)?;
    let filtered = filter_claims(&claims, args.claim.as_deref())?;
    let output = LintOutput {
        ok: true,
        claim_count: filtered.len(),
        claims: filtered
            .iter()
            .map(|claim| LintClaimOutput {
                claim_id: claim.claim.id.clone(),
                title: claim.claim.title.clone(),
                revision_digest: claim.claim.revision_digest.clone(),
            })
            .collect(),
        verify_commands: config
            .verify
            .commands
            .iter()
            .map(|command| command.command().to_string())
            .collect(),
    };
    write_lint(stdout, OutputMode::from_json_flag(args.json), &output)?;
    Ok(CliExit::Success)
}

fn dispatch_verify(
    config: &CanonicalTriadConfig,
    args: VerifyArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let claims = load_claims(config)?;
    let claim_id = parse_claim_id(&args.claim)?;
    let claim = find_claim(&claims, &claim_id)?;
    let artifact_digests = SnapshotAdapter::collect(&config.repo_root, &config.snapshot.include)?;

    let mut appended_ids = Vec::new();
    for command in resolve_verify_commands(config, claim)? {
        let evidence_artifacts =
            SnapshotAdapter::filter(&artifact_digests, &command.artifact_include);
        let evidence =
            EvidenceNdjsonStore::append_new(&config.paths.evidence_file, move |evidence_id| {
                CommandCapture::capture(
                    &config.repo_root,
                    &claim.claim,
                    evidence_id,
                    &command.command,
                    command.locator.as_deref(),
                    evidence_artifacts,
                )
            })?;
        appended_ids.push(evidence.id.clone());
    }

    let evidence = EvidenceNdjsonStore::read(&config.paths.evidence_file)?;
    let report = verify_claim(&claim.claim, &artifact_digests, &evidence);
    let exit = CliExit::for_claim_report(&report);
    let output = VerifyOutput {
        claim_id: claim.claim.id.clone(),
        evidence_ids: appended_ids,
        report,
    };
    write_verify(stdout, OutputMode::from_json_flag(args.json), &output)?;
    Ok(exit)
}

fn dispatch_report(
    config: &CanonicalTriadConfig,
    args: ReportArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let claims = load_claims(config)?;
    let artifact_digests = SnapshotAdapter::collect(&config.repo_root, &config.snapshot.include)?;
    let evidence = EvidenceNdjsonStore::read(&config.paths.evidence_file)?;

    let reports = if let Some(claim) = args.claim.as_deref() {
        let claim_id = parse_claim_id(claim)?;
        let claim = find_claim(&claims, &claim_id)?;
        vec![verify_claim(&claim.claim, &artifact_digests, &evidence)]
    } else {
        claims
            .iter()
            .map(|claim| verify_claim(&claim.claim, &artifact_digests, &evidence))
            .collect()
    };

    let exit = CliExit::for_reports(&reports);
    write_report(stdout, OutputMode::from_json_flag(args.json), &reports)?;
    Ok(exit)
}

fn load_claims(config: &CanonicalTriadConfig) -> Result<Vec<LoadedClaim>> {
    ClaimMarkdownAdapter::discover_claim_file_paths(&config.paths.claim_dir)?
        .iter()
        .map(|path| {
            Ok::<LoadedClaim, triad_core::TriadError>(LoadedClaim {
                claim: ClaimMarkdownAdapter::parse_claim_file(path)?,
                path: path.clone(),
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(anyhow::Error::from)
}

fn filter_claims<'a>(
    claims: &'a [LoadedClaim],
    claim_id: Option<&str>,
) -> Result<Vec<&'a LoadedClaim>> {
    if let Some(claim_id) = claim_id {
        let claim_id = parse_claim_id(claim_id)?;
        Ok(vec![find_claim(claims, &claim_id)?])
    } else {
        Ok(claims.iter().collect())
    }
}

fn find_claim<'a>(claims: &'a [LoadedClaim], claim_id: &ClaimId) -> Result<&'a LoadedClaim> {
    claims
        .iter()
        .find(|claim| &claim.claim.id == claim_id)
        .ok_or_else(|| TriadError::InvalidState(format!("claim not found: {claim_id}")).into())
}

fn resolve_verify_commands(
    config: &CanonicalTriadConfig,
    claim: &LoadedClaim,
) -> Result<Vec<ResolvedVerifyCommand>> {
    let claim_path = claim
        .path
        .strip_prefix(&config.repo_root)
        .map_err(|_| anyhow!("claim path escaped repo root: {}", claim.path))?
        .as_str()
        .to_string();

    Ok(config
        .verify
        .commands
        .iter()
        .map(|entry| ResolvedVerifyCommand {
            command: expand_template(entry.command(), &claim.claim.id, &claim_path),
            locator: entry
                .locator()
                .map(|locator| expand_template(locator, &claim.claim.id, &claim_path)),
            artifact_include: entry
                .artifacts()
                .map(|artifacts| artifacts.to_vec())
                .unwrap_or_else(|| config.snapshot.include.clone()),
        })
        .collect())
}

fn expand_template(template: &str, claim_id: &ClaimId, claim_path: &str) -> String {
    template
        .replace("{claim_id}", claim_id.as_str())
        .replace("{claim_path}", claim_path)
}
