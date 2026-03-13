use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use triad_core::{ClaimId, ClaimReport, EvidenceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    pub(crate) fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct InitOutput {
    pub repo_root: String,
    pub config_path: String,
    pub evidence_file: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct LintClaimOutput {
    pub claim_id: ClaimId,
    pub title: String,
    pub revision_digest: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct LintOutput {
    pub ok: bool,
    pub claim_count: usize,
    pub claims: Vec<LintClaimOutput>,
    pub verify_commands: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct VerifyOutput {
    pub claim_id: ClaimId,
    pub evidence_ids: Vec<EvidenceId>,
    pub report: ClaimReport,
}

pub(crate) fn write_init(
    stdout: &mut impl Write,
    mode: OutputMode,
    output: &InitOutput,
) -> Result<()> {
    match mode {
        OutputMode::Human => {
            writeln!(stdout, "initialized triad scaffold")?;
            writeln!(stdout, "Config: {}", output.config_path)?;
            writeln!(stdout, "Evidence: {}", output.evidence_file)?;
        }
        OutputMode::Json => write_json(stdout, output)?,
    }
    Ok(())
}

pub(crate) fn write_lint(
    stdout: &mut impl Write,
    mode: OutputMode,
    output: &LintOutput,
) -> Result<()> {
    match mode {
        OutputMode::Human => {
            writeln!(stdout, "lint ok")?;
            writeln!(stdout, "Claims: {}", output.claim_count)?;
            for claim in &output.claims {
                writeln!(stdout, "{}  {}", claim.claim_id, claim.title)?;
            }
        }
        OutputMode::Json => write_json(stdout, output)?,
    }
    Ok(())
}

pub(crate) fn write_verify(
    stdout: &mut impl Write,
    mode: OutputMode,
    output: &VerifyOutput,
) -> Result<()> {
    match mode {
        OutputMode::Human => {
            writeln!(stdout, "{}  {}", output.claim_id, output.report.status)?;
            writeln!(stdout, "Evidence: {}", render_ids(&output.evidence_ids))?;
        }
        OutputMode::Json => write_json(stdout, output)?,
    }
    Ok(())
}

pub(crate) fn write_report(
    stdout: &mut impl Write,
    mode: OutputMode,
    reports: &[ClaimReport],
) -> Result<()> {
    match mode {
        OutputMode::Human => {
            for report in reports {
                writeln!(stdout, "{}  {}", report.claim_id, report.status)?;
            }
        }
        OutputMode::Json => write_json(stdout, reports)?,
    }
    Ok(())
}

fn write_json(stdout: &mut impl Write, value: &(impl Serialize + ?Sized)) -> Result<()> {
    serde_json::to_writer_pretty(&mut *stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}

fn render_ids(ids: &[EvidenceId]) -> String {
    if ids.is_empty() {
        "none".into()
    } else {
        ids.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
