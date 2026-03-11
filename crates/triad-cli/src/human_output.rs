use std::io::Write;

use triad_core::{
    ApplyPatchReport, ClaimId, ClaimSummary, DriftStatus, NextAction, NextClaim, PatchId,
    RunClaimReport, StatusReport, StatusSummary, Verdict, VerifyLayer, VerifyReport,
};

pub(crate) fn write_work(stdout: &mut impl Write, report: &RunClaimReport) -> anyhow::Result<()> {
    write!(stdout, "{}", render_work(report))?;
    Ok(())
}

pub(crate) fn write_next(
    stdout: &mut impl Write,
    next: &NextClaim,
    diagnostics: &[String],
) -> anyhow::Result<()> {
    write!(stdout, "{}", render_next(next, diagnostics))?;
    Ok(())
}

pub(crate) fn write_status(
    stdout: &mut impl Write,
    report: &StatusReport,
    claim_filter: Option<&ClaimId>,
    verbose: bool,
    diagnostics: &[String],
) -> anyhow::Result<()> {
    write!(
        stdout,
        "{}",
        render_status(report, claim_filter, verbose, diagnostics)
    )?;
    Ok(())
}

pub(crate) fn write_verify(stdout: &mut impl Write, report: &VerifyReport) -> anyhow::Result<()> {
    write!(stdout, "{}", render_verify(report))?;
    Ok(())
}

pub(crate) fn write_accept(
    stdout: &mut impl Write,
    report: &ApplyPatchReport,
) -> anyhow::Result<()> {
    write!(stdout, "{}", render_accept(report))?;
    Ok(())
}

fn render_work(report: &RunClaimReport) -> String {
    let patch_state = if report.needs_patch {
        "required"
    } else {
        "not-required"
    };

    [
        format!("{}  work", report.claim_id),
        format!("Summary: {}", report.summary),
        format!("Run: {}", report.run_id),
        format!(
            "Changed paths: {}",
            render_string_list(&report.changed_paths)
        ),
        format!(
            "Suggested tests: {}",
            render_string_list(&report.suggested_test_selectors)
        ),
        format!("Patch draft: {patch_state}"),
        format!("Blockers: {}", render_string_list(&report.blocked_actions)),
        String::new(),
        format!("Next: triad verify {}", report.claim_id),
        String::new(),
    ]
    .join("\n")
}

fn render_next(next: &NextClaim, diagnostics: &[String]) -> String {
    let suggested = command_for_next_action(&next.claim_id, next.next_action, None);
    let mut lines = vec![
        format!("{}  {}", next.claim_id, status_label(next.status)),
        format!("Reason: {}", next.reason),
        format!("Suggested: {suggested}"),
    ];
    append_claim_diagnostics(&mut lines, diagnostics);
    lines.push(String::new());
    lines.push(format!("Next: {suggested}"));
    lines.push(String::new());
    lines.join("\n")
}

fn render_status(
    report: &StatusReport,
    claim_filter: Option<&ClaimId>,
    verbose: bool,
    diagnostics: &[String],
) -> String {
    let recommended = recommended_claim(report, claim_filter);
    let claims_to_render = claims_to_render(report, claim_filter, verbose, recommended);

    let mut lines = vec![
        format!("Claims: {}", report.claims.len()),
        render_status_summary(&report.summary),
    ];

    if !claims_to_render.is_empty() {
        lines.push(String::new());
    }

    for claim in claims_to_render {
        lines.extend(render_claim_summary(claim));
        lines.push(String::new());
    }

    append_claim_diagnostics(&mut lines, diagnostics);
    let next_command = recommended
        .map(command_for_claim_summary)
        .unwrap_or_else(|| "triad next".to_string());
    lines.push(format!("Next: {next_command}"));
    lines.push(String::new());

    lines.join("\n")
}

fn render_verify(report: &VerifyReport) -> String {
    let blockers = verify_blockers(report);
    let next_command = command_for_drift_status(
        &report.claim_id,
        report.status_after_verify,
        report.pending_patch_id.as_ref(),
    );

    [
        format!(
            "{}  verify  {}",
            report.claim_id,
            verdict_label(report.verdict)
        ),
        format!(
            "Summary: status -> {}",
            status_label(report.status_after_verify)
        ),
        format!("Layers: {}", render_verify_layers(&report.layers)),
        format!(
            "Scope: {}",
            if report.full_workspace {
                "full-workspace"
            } else {
                "targeted"
            }
        ),
        format!("Evidence: {}", render_id_list(&report.evidence_ids)),
        format!("Blockers: {}", render_string_list(&blockers)),
        String::new(),
        format!("Next: {next_command}"),
        String::new(),
    ]
    .join("\n")
}

fn render_accept(report: &ApplyPatchReport) -> String {
    let blockers = if report.applied {
        "none".to_string()
    } else {
        "patch not applied".to_string()
    };

    [
        format!("{}  accept", report.patch_id),
        format!(
            "Summary: {} for {}",
            if report.applied {
                "applied"
            } else {
                "not-applied"
            },
            report.claim_id
        ),
        format!("New revision: {}", report.new_revision),
        format!("Blockers: {blockers}"),
        String::new(),
        format!(
            "Next: {}",
            command_for_next_action(
                &report.claim_id,
                report.followup_action,
                Some(&report.patch_id),
            )
        ),
        String::new(),
    ]
    .join("\n")
}

fn claims_to_render<'a>(
    report: &'a StatusReport,
    claim_filter: Option<&ClaimId>,
    verbose: bool,
    recommended: Option<&'a ClaimSummary>,
) -> Vec<&'a ClaimSummary> {
    if claim_filter.is_some() || verbose {
        return report.claims.iter().collect();
    }

    recommended.into_iter().collect()
}

fn recommended_claim<'a>(
    report: &'a StatusReport,
    claim_filter: Option<&ClaimId>,
) -> Option<&'a ClaimSummary> {
    if let Some(claim_id) = claim_filter {
        return report
            .claims
            .iter()
            .find(|claim| &claim.claim_id == claim_id);
    }

    report.claims.iter().min_by(|left, right| {
        claim_priority(left)
            .cmp(&claim_priority(right))
            .then_with(|| left.claim_id.as_str().cmp(right.claim_id.as_str()))
    })
}

fn render_status_summary(summary: &StatusSummary) -> String {
    format!(
        "Healthy: {}  Needs-code: {}  Needs-test: {}  Needs-spec: {}  Contradicted: {}  Blocked: {}",
        summary.healthy,
        summary.needs_code,
        summary.needs_test,
        summary.needs_spec,
        summary.contradicted,
        summary.blocked,
    )
}

fn render_claim_summary(claim: &ClaimSummary) -> Vec<String> {
    let mut lines = vec![format!(
        "{}  {}  {}",
        claim.claim_id,
        status_label(claim.status),
        claim.title
    )];

    if let Some(patch_id) = claim.pending_patch_id.as_ref() {
        lines.push(format!("Pending patch: {patch_id}"));
    }

    lines.push(format!("Suggested: {}", command_for_claim_summary(claim)));
    lines
}

fn append_claim_diagnostics(lines: &mut Vec<String>, diagnostics: &[String]) {
    if diagnostics.is_empty() {
        return;
    }

    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push("Problems:".to_string());
    lines.extend(
        diagnostics
            .iter()
            .map(|diagnostic| format!("- {diagnostic}")),
    );
}

fn command_for_claim_summary(claim: &ClaimSummary) -> String {
    command_for_drift_status(
        &claim.claim_id,
        claim.status,
        claim.pending_patch_id.as_ref(),
    )
}

fn command_for_drift_status(
    claim_id: &ClaimId,
    status: DriftStatus,
    patch_id: Option<&PatchId>,
) -> String {
    let next_action = match status {
        DriftStatus::Contradicted | DriftStatus::NeedsCode => NextAction::Work,
        DriftStatus::NeedsTest => NextAction::Verify,
        DriftStatus::NeedsSpec if patch_id.is_some() => NextAction::Accept,
        DriftStatus::NeedsSpec | DriftStatus::Blocked | DriftStatus::Healthy => NextAction::Status,
    };

    command_for_next_action(claim_id, next_action, patch_id)
}

fn verify_blockers(report: &VerifyReport) -> Vec<String> {
    let mut blockers = Vec::new();

    match report.verdict {
        Verdict::Pass => {}
        Verdict::Fail => blockers.push("verification failed".to_string()),
        Verdict::Unknown => blockers.push("verification inconclusive".to_string()),
    }

    if let Some(patch_id) = report.pending_patch_id.as_ref() {
        blockers.push(format!("pending patch: {patch_id}"));
    }

    blockers
}

fn command_for_next_action(
    claim_id: &ClaimId,
    action: NextAction,
    patch_id: Option<&PatchId>,
) -> String {
    match action {
        NextAction::Work => format!("triad work {claim_id}"),
        NextAction::Verify => format!("triad verify {claim_id}"),
        NextAction::Accept => patch_id
            .map(|id| format!("triad accept {id}"))
            .unwrap_or_else(|| format!("triad status --claim {claim_id}")),
        NextAction::Status => format!("triad status --claim {claim_id}"),
    }
}

fn claim_priority(claim: &ClaimSummary) -> u8 {
    match claim.status {
        DriftStatus::Contradicted => 0,
        DriftStatus::NeedsTest => 1,
        DriftStatus::NeedsCode => 2,
        DriftStatus::NeedsSpec => 3,
        DriftStatus::Blocked => 4,
        DriftStatus::Healthy => 5,
    }
}

fn status_label(status: DriftStatus) -> &'static str {
    match status {
        DriftStatus::Healthy => "healthy",
        DriftStatus::NeedsCode => "needs-code",
        DriftStatus::NeedsTest => "needs-test",
        DriftStatus::NeedsSpec => "needs-spec",
        DriftStatus::Contradicted => "contradicted",
        DriftStatus::Blocked => "blocked",
    }
}

fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "pass",
        Verdict::Fail => "fail",
        Verdict::Unknown => "unknown",
    }
}

fn render_verify_layers(layers: &[VerifyLayer]) -> String {
    if layers.is_empty() {
        return "none".to_string();
    }

    layers
        .iter()
        .map(|layer| match layer {
            VerifyLayer::Unit => "unit",
            VerifyLayer::Contract => "contract",
            VerifyLayer::Integration => "integration",
            VerifyLayer::Probe => "probe",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_id_list<T: std::fmt::Display>(items: &[T]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn render_string_list(items: &[String]) -> String {
    render_id_list(items)
}
