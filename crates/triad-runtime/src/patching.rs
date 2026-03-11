use std::fs;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{
    ApplyPatchReport, Claim, ClaimId, NextAction, PatchDraft, PatchId, PatchState, TriadApi,
    TriadError, Verdict,
};

use crate::claims::{
    canonical_claim_lines, claim_md_path, claim_revision_digest, parse_claim_file,
    parsed_claim_by_id_or_issue,
};
use crate::drift::next_action_for_status;
use crate::repo_support::{repo_relative_utf8, resolve_repo_relative_path, utf8_path};
use crate::storage::{read_patch_meta, update_patch_draft_state};
use crate::verify_exec::{
    latest_relevant_evidence, latest_run_record_for_claim, normalized_run_changed_paths,
    verify_claim_with_runner,
};
use crate::{LocalTriad, VerifyCommandRunner};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeterministicMismatch {
    pub(crate) claim_id: ClaimId,
    pub(crate) claim_path: Utf8PathBuf,
    pub(crate) based_on_evidence: Vec<triad_core::EvidenceId>,
    pub(crate) run_id: triad_core::RunId,
    pub(crate) changed_paths: Vec<Utf8PathBuf>,
    pub(crate) summary: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUnifiedDiff {
    path: Utf8PathBuf,
    hunk: ParsedUnifiedHunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedUnifiedHunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

pub(crate) fn deterministic_mismatch_for_claim(
    triad: &LocalTriad,
    claim_id: &ClaimId,
) -> Result<Option<DeterministicMismatch>, TriadError> {
    let claim = parsed_claim_by_id_or_issue(triad, claim_id)?;
    let relevant = triad.relevant_evidence_for_claim(claim_id)?;
    let Some(latest_evidence) = latest_relevant_evidence(&relevant) else {
        return Ok(None);
    };
    if latest_evidence.verdict != Verdict::Pass {
        return Ok(None);
    }

    let Some(run_record) =
        latest_run_record_for_claim(triad.config.paths.run_dir.as_std_path(), claim_id)?
    else {
        return Ok(None);
    };
    if !run_record.needs_patch {
        return Ok(None);
    }

    let summary = run_record.summary.trim();
    if summary.is_empty() {
        return Ok(None);
    }

    let mut overlapping_paths = normalized_run_changed_paths(&run_record)?
        .filter_map(|result| match result {
            Ok(path)
                if latest_evidence
                    .covered_paths
                    .iter()
                    .any(|covered| covered == &path) =>
            {
                Some(Ok(path))
            }
            Ok(_) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>, TriadError>>()?;
    if overlapping_paths.is_empty() {
        return Ok(None);
    }
    overlapping_paths.sort();

    let overlap_display = overlapping_paths
        .iter()
        .map(|path| path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mismatch = DeterministicMismatch {
        claim_id: claim.id.clone(),
        claim_path: claim_md_path(&triad.config.paths.claim_dir, &claim.id),
        based_on_evidence: vec![latest_evidence.id.clone()],
        run_id: run_record.run_id.clone(),
        changed_paths: overlapping_paths,
        summary: summary.to_string(),
        reason: format!(
            "latest run {} marked needs_patch after fresh pass evidence {} on {}: {}",
            run_record.run_id, latest_evidence.id, overlap_display, summary
        ),
    };

    Ok(Some(mismatch))
}

pub(crate) fn proposed_claim_for_mismatch(
    current: &Claim,
    mismatch: &DeterministicMismatch,
) -> Claim {
    let behavior_note = format!("Behavior update: {}", mismatch.summary.trim());
    let notes = match current
        .notes
        .as_ref()
        .map(|notes| notes.trim())
        .filter(|notes| !notes.is_empty())
    {
        Some(existing) if existing.lines().any(|line| line.trim() == behavior_note) => {
            existing.to_string()
        }
        Some(existing) => format!("{existing}\n{behavior_note}"),
        None => behavior_note,
    };

    Claim {
        id: current.id.clone(),
        title: current.title.clone(),
        statement: current.statement.clone(),
        examples: current.examples.clone(),
        invariants: current.invariants.clone(),
        notes: Some(notes),
        revision: current.revision,
    }
}

pub(crate) fn apply_patch_with_runner<R: VerifyCommandRunner>(
    triad: &LocalTriad,
    id: &PatchId,
    runner: &R,
) -> Result<ApplyPatchReport, TriadError> {
    let meta = read_patch_meta(triad.config.paths.patch_dir.as_std_path(), id)?;
    let draft = triad.read_patch_draft(id)?;
    if draft.state != PatchState::Pending {
        return Err(TriadError::InvalidState(format!(
            "patch {} is not pending",
            id
        )));
    }

    let claim_id = apply_patch_draft_to_claim(
        triad.config.repo_root.as_std_path(),
        &triad.config.paths.claim_dir,
        meta.base_claim_digest.as_deref(),
        &draft,
    )?;
    let accepted_claim = parsed_claim_by_id_or_issue(triad, &claim_id)?;
    update_patch_draft_state(
        triad.config.repo_root.as_std_path(),
        triad.config.paths.patch_dir.as_std_path(),
        id,
        PatchState::Applied,
    )?;

    let followup_action = if triad.config.verify.full_workspace_after_accept {
        let verify_request = triad.default_verify_request(claim_id.clone(), false, true)?;
        verify_claim_with_runner(triad, verify_request, runner)?;
        next_action_for_status(triad.detect_drift(&claim_id)?.status)
    } else {
        NextAction::Verify
    };

    Ok(ApplyPatchReport {
        patch_id: id.clone(),
        claim_id,
        applied: true,
        new_revision: accepted_claim.revision,
        followup_action,
    })
}

pub(crate) fn minimal_claim_diff(
    claim_path: &Utf8Path,
    current: &Claim,
    proposed: &Claim,
) -> Result<String, TriadError> {
    if current.id != proposed.id {
        return Err(TriadError::InvalidState(format!(
            "patch diff claim ids do not match: {} != {}",
            current.id, proposed.id
        )));
    }

    let old_lines = canonical_claim_lines(current);
    let new_lines = canonical_claim_lines(proposed);

    let prefix_len = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(left, right)| left == right)
        .count();

    if prefix_len == old_lines.len() && prefix_len == new_lines.len() {
        return Err(TriadError::InvalidState(format!(
            "no claim changes to diff for {}",
            current.id
        )));
    }

    let old_remaining = old_lines.len() - prefix_len;
    let new_remaining = new_lines.len() - prefix_len;
    let mut suffix_len = 0usize;
    while suffix_len < old_remaining
        && suffix_len < new_remaining
        && old_lines[old_lines.len() - 1 - suffix_len]
            == new_lines[new_lines.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let old_changed = &old_lines[prefix_len..old_lines.len() - suffix_len];
    let new_changed = &new_lines[prefix_len..new_lines.len() - suffix_len];

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n", claim_path));
    diff.push_str(&format!("+++ b/{}\n", claim_path));
    diff.push_str(&format!(
        "@@ -{} +{} @@\n",
        unified_range(prefix_len, old_changed.len()),
        unified_range(prefix_len, new_changed.len())
    ));

    for line in old_changed {
        diff.push('-');
        diff.push_str(line);
        diff.push('\n');
    }
    for line in new_changed {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }

    Ok(diff)
}

fn parse_unified_diff(diff: &str) -> Result<ParsedUnifiedDiff, TriadError> {
    let lines = diff.lines().collect::<Vec<_>>();
    if lines.len() < 3 {
        return Err(TriadError::InvalidState(
            "unified diff must contain header and hunk".to_string(),
        ));
    }

    let old_path = lines[0].strip_prefix("--- a/").ok_or_else(|| {
        TriadError::InvalidState("unified diff is missing old path header".into())
    })?;
    let new_path = lines[1].strip_prefix("+++ b/").ok_or_else(|| {
        TriadError::InvalidState("unified diff is missing new path header".into())
    })?;
    if old_path != new_path {
        return Err(TriadError::InvalidState(format!(
            "unified diff paths do not match: {} != {}",
            old_path, new_path
        )));
    }

    let hunk_header = lines[2]
        .strip_prefix("@@ -")
        .and_then(|line| line.strip_suffix(" @@"))
        .ok_or_else(|| TriadError::InvalidState("unified diff is missing hunk header".into()))?;
    let (old_range, new_range) = hunk_header.split_once(" +").ok_or_else(|| {
        TriadError::InvalidState(format!("invalid unified diff hunk header: {}", lines[2]))
    })?;
    let (old_start, old_count) = parse_unified_range(old_range)?;
    let (new_start, new_count) = parse_unified_range(new_range)?;

    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    for line in &lines[3..] {
        if let Some(line) = line.strip_prefix('-') {
            old_lines.push(line.to_string());
        } else if let Some(line) = line.strip_prefix('+') {
            new_lines.push(line.to_string());
        } else {
            return Err(TriadError::InvalidState(format!(
                "unsupported unified diff line: {}",
                line
            )));
        }
    }

    if old_lines.len() != old_count || new_lines.len() != new_count {
        return Err(TriadError::InvalidState(format!(
            "unified diff hunk counts do not match payload: -{}/+{} vs -{}/+{}",
            old_count,
            new_count,
            old_lines.len(),
            new_lines.len()
        )));
    }

    Ok(ParsedUnifiedDiff {
        path: Utf8PathBuf::from(old_path),
        hunk: ParsedUnifiedHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            old_lines,
            new_lines,
        },
    })
}

fn parse_unified_range(value: &str) -> Result<(usize, usize), TriadError> {
    if let Some((start, count)) = value.split_once(',') {
        let start = start.parse::<usize>().map_err(|err| {
            TriadError::InvalidState(format!("invalid unified diff range {}: {err}", value))
        })?;
        let count = count.parse::<usize>().map_err(|err| {
            TriadError::InvalidState(format!("invalid unified diff range {}: {err}", value))
        })?;
        Ok((start, count))
    } else {
        let start = value.parse::<usize>().map_err(|err| {
            TriadError::InvalidState(format!("invalid unified diff range {}: {err}", value))
        })?;
        Ok((start, 1))
    }
}

fn apply_patch_draft_to_claim(
    repo_root: &Path,
    claim_dir: &Utf8Path,
    base_claim_digest: Option<&str>,
    draft: &PatchDraft,
) -> Result<ClaimId, TriadError> {
    let diff = parse_unified_diff(&draft.unified_diff)?;
    let expected_path = repo_relative_utf8(
        repo_root,
        claim_md_path(claim_dir, &draft.claim_id).as_std_path(),
    )?;
    if diff.path != expected_path {
        return Err(TriadError::InvalidState(format!(
            "patch diff path does not match claim {}: {}",
            draft.claim_id, diff.path
        )));
    }

    let claim_path = resolve_repo_relative_path(repo_root, &diff.path)?;
    let current = parse_claim_file(&utf8_path(claim_path.clone(), "claim path")?)?;
    if let Some(expected_digest) = base_claim_digest {
        let current_digest = claim_revision_digest(&current);
        if current_digest != expected_digest {
            return Err(TriadError::patch_conflict(
                draft.id.as_str(),
                &format!("claim file no longer matches {}", diff.path),
            ));
        }
    }
    let current_lines = canonical_claim_lines(&current);
    let start_index = if diff.hunk.old_count == 0 {
        diff.hunk.old_start
    } else {
        diff.hunk.old_start.checked_sub(1).ok_or_else(|| {
            TriadError::InvalidState("unified diff old range must start at >= 1".into())
        })?
    };
    let end_index = start_index + diff.hunk.old_count;

    if end_index > current_lines.len()
        || current_lines[start_index..end_index] != diff.hunk.old_lines
    {
        return Err(TriadError::patch_conflict(
            draft.id.as_str(),
            &format!("claim file no longer matches {}", diff.path),
        ));
    }

    let mut next_lines = Vec::new();
    next_lines.extend_from_slice(&current_lines[..start_index]);
    next_lines.extend(diff.hunk.new_lines.iter().cloned());
    next_lines.extend_from_slice(&current_lines[end_index..]);

    let mut next_text = next_lines.join("\n");
    next_text.push('\n');
    fs::write(&claim_path, next_text).map_err(|err| {
        TriadError::Io(format!(
            "failed to write patched claim file {}: {err}",
            claim_path.display()
        ))
    })?;

    Ok(current.id)
}

fn unified_range(prefix_len: usize, count: usize) -> String {
    match count {
        0 => format!("{prefix_len},0"),
        1 => (prefix_len + 1).to_string(),
        _ => format!("{},{}", prefix_len + 1, count),
    }
}
