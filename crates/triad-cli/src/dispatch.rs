use std::collections::BTreeMap;
use std::io::Write;

use anyhow::{Result, anyhow};
use camino::Utf8Path;
use triad_core::{Claim, ClaimId, verify_claim, verify_many};
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

pub(crate) fn dispatch_command(
    config: &CanonicalTriadConfig,
    command: Command,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    match command {
        Command::Init(_) => Err(anyhow!("init must be handled before runtime dispatch")),
        Command::Lint(args) => dispatch_lint(config, args, stdout),
        Command::Verify(args) => dispatch_verify(config, args, stdout),
        Command::Report(args) => dispatch_report(config, args, stdout),
    }
}

pub(crate) fn dispatch_init(
    repo_root: &Utf8Path,
    output_mode: OutputMode,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let output = InitOutput {
        repo_root: repo_root.as_str().to_string(),
        config_path: repo_root.join("triad.toml").as_str().to_string(),
        evidence_file: repo_root
            .join(".triad/evidence.ndjson")
            .as_str()
            .to_string(),
    };
    write_init(stdout, output_mode, &output)?;
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
                claim_id: claim.id.clone(),
                title: claim.title.clone(),
                revision_digest: claim.revision_digest.clone(),
            })
            .collect(),
        verify_commands: config.verify.commands.clone(),
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
    for command in &config.verify.commands {
        let evidence_id = EvidenceNdjsonStore::next_evidence_id(&config.paths.evidence_file)?;
        let evidence = CommandCapture::capture(
            claim,
            evidence_id.clone(),
            command,
            artifact_digests.clone(),
        )?;
        EvidenceNdjsonStore::append(&config.paths.evidence_file, &evidence)?;
        appended_ids.push(evidence_id);
    }

    let evidence = EvidenceNdjsonStore::read(&config.paths.evidence_file)?;
    let report = verify_claim(claim, &artifact_digests, &evidence);
    let exit = CliExit::for_claim_report(&report);
    let output = VerifyOutput {
        claim_id: claim.id.clone(),
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
        vec![verify_claim(claim, &artifact_digests, &evidence)]
    } else {
        let snapshots = claims
            .iter()
            .map(|claim| (claim.id.clone(), artifact_digests.clone()))
            .collect::<BTreeMap<ClaimId, BTreeMap<String, String>>>();
        verify_many(&claims, &snapshots, &evidence)
    };

    let exit = CliExit::for_reports(&reports);
    write_report(stdout, OutputMode::from_json_flag(args.json), &reports)?;
    Ok(exit)
}

fn load_claims(config: &CanonicalTriadConfig) -> Result<Vec<Claim>> {
    ClaimMarkdownAdapter::discover_claim_file_paths(&config.paths.claim_dir)?
        .iter()
        .map(|path| ClaimMarkdownAdapter::parse_claim_file(path))
        .collect::<Result<Vec<_>, _>>()
        .map_err(anyhow::Error::from)
}

fn filter_claims<'a>(claims: &'a [Claim], claim_id: Option<&str>) -> Result<Vec<&'a Claim>> {
    if let Some(claim_id) = claim_id {
        let claim_id = parse_claim_id(claim_id)?;
        Ok(vec![find_claim(claims, &claim_id)?])
    } else {
        Ok(claims.iter().collect())
    }
}

fn find_claim<'a>(claims: &'a [Claim], claim_id: &ClaimId) -> Result<&'a Claim> {
    claims
        .iter()
        .find(|claim| &claim.id == claim_id)
        .ok_or_else(|| anyhow!("claim not found: {claim_id}"))
}
