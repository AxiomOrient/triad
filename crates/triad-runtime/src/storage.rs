use std::collections::HashMap;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use triad_core::{
    ClaimId, Evidence, EvidenceId, PatchDraft, PatchId, PatchState, RunClaimReport, RunId,
    TriadError,
};

use crate::RunRecord;
use crate::claims::claim_base_digest;
use crate::fs_support::ensure_dir;
use crate::repo_support::{
    normalize_serde_row_error, patch_diff_path, patch_json_path, repo_relative_utf8,
    resolve_repo_relative_path, run_json_path,
};

#[derive(Deserialize)]
pub(crate) struct EvidenceIdRow {
    pub(crate) id: EvidenceId,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PatchDraftMeta {
    pub(crate) id: PatchId,
    pub(crate) claim_id: ClaimId,
    pub(crate) based_on_evidence: Vec<EvidenceId>,
    pub(crate) rationale: String,
    pub(crate) created_at: String,
    pub(crate) state: PatchState,
    pub(crate) diff_path: camino::Utf8PathBuf,
    pub(crate) base_claim_digest: Option<String>,
}

pub(crate) fn next_evidence_id(path: &Path) -> Result<EvidenceId, TriadError> {
    let content = fs::read_to_string(path).map_err(|err| {
        TriadError::Io(format!(
            "failed to read evidence log {}: {err}",
            path.display()
        ))
    })?;
    let max_sequence = ndjson_rows_from_str::<EvidenceIdRow>(&content, "evidence row", path)?
        .into_iter()
        .map(|row| row.id.sequence_number())
        .max()
        .unwrap_or(0);
    EvidenceId::from_sequence(max_sequence + 1)
}

pub(crate) fn append_evidence(path: &Path, evidence: &Evidence) -> Result<(), TriadError> {
    let content = fs::read_to_string(path).map_err(|err| {
        TriadError::Io(format!(
            "failed to read evidence log {}: {err}",
            path.display()
        ))
    })?;

    if !content.is_empty() && !content.ends_with('\n') {
        return Err(TriadError::InvalidState(format!(
            "evidence log must end with newline before append: {}",
            path.display()
        )));
    }

    let max_sequence = ndjson_rows_from_str::<EvidenceIdRow>(&content, "evidence row", path)?
        .into_iter()
        .map(|row| row.id.sequence_number())
        .max()
        .unwrap_or(0);
    let expected_id = EvidenceId::from_sequence(max_sequence + 1)?;

    if evidence.id != expected_id {
        return Err(TriadError::InvalidState(format!(
            "evidence id must be next monotonic id for {}: expected {}, got {}",
            path.display(),
            expected_id,
            evidence.id
        )));
    }

    let line = serde_json::to_string(evidence).map_err(|err| {
        TriadError::Serialization(format!(
            "failed to serialize evidence {}: {}",
            evidence.id, err
        ))
    })?;
    let mut file = OpenOptions::new().append(true).open(path).map_err(|err| {
        TriadError::Io(format!(
            "failed to open evidence log {}: {err}",
            path.display()
        ))
    })?;
    use std::io::Write;
    writeln!(file, "{line}").map_err(|err| {
        TriadError::Io(format!(
            "failed to append evidence to {}: {err}",
            path.display()
        ))
    })
}

pub(crate) fn read_evidence(path: &Path) -> Result<Vec<Evidence>, TriadError> {
    read_ndjson_rows(path, "evidence row")
}

pub(crate) fn read_ndjson_rows<T>(path: &Path, row_kind: &str) -> Result<Vec<T>, TriadError>
where
    T: DeserializeOwned,
{
    let content = fs::read_to_string(path).map_err(|err| {
        TriadError::Io(format!(
            "failed to read evidence log {}: {err}",
            path.display()
        ))
    })?;
    ndjson_rows_from_str(&content, row_kind, path)
}

fn ndjson_rows_from_str<T>(content: &str, row_kind: &str, path: &Path) -> Result<Vec<T>, TriadError>
where
    T: DeserializeOwned,
{
    let mut rows = Vec::new();

    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let row = serde_json::from_str(line).map_err(|err| {
            TriadError::Serialization(format!(
                "invalid {row_kind} at line {} in {}: {}",
                index + 1,
                path.display(),
                normalize_serde_row_error(&err.to_string())
            ))
        })?;
        rows.push(row);
    }

    Ok(rows)
}

pub(crate) fn next_run_id(run_dir: &Path) -> Result<RunId, TriadError> {
    let mut max_sequence = 0u32;
    for path in json_files_in_dir(run_dir)? {
        let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
            TriadError::InvalidState(format!(
                "run record path must be valid UTF-8: {}",
                path.display()
            ))
        })?;
        let run_id = RunId::new(stem).map_err(|_| {
            TriadError::InvalidState(format!("invalid run record name: {}", path.display()))
        })?;
        max_sequence = max_sequence.max(run_id.sequence_number());
    }
    RunId::from_sequence(max_sequence + 1)
}

pub(crate) fn store_run_record(
    run_dir: &Path,
    report: &RunClaimReport,
    prompt_fingerprint: &str,
    runtime_metadata: &std::collections::BTreeMap<String, String>,
) -> Result<(), TriadError> {
    let expected_id = next_run_id(run_dir)?;
    if report.run_id != expected_id {
        return Err(TriadError::InvalidState(format!(
            "run id must be next monotonic id for {}: expected {}, got {}",
            run_dir.display(),
            expected_id,
            report.run_id
        )));
    }

    let record = RunRecord {
        run_id: report.run_id.clone(),
        claim_id: report.claim_id.clone(),
        summary: report.summary.clone(),
        changed_paths: report.changed_paths.clone(),
        suggested_test_selectors: report.suggested_test_selectors.clone(),
        blocked_actions: report.blocked_actions.clone(),
        needs_patch: report.needs_patch,
        prompt_fingerprint: prompt_fingerprint.to_string(),
        runtime_metadata: runtime_metadata.clone(),
    };
    let path = run_json_path(run_dir, &record.run_id);
    let json = serde_json::to_string_pretty(&record).map_err(|err| {
        TriadError::Serialization(format!(
            "failed to serialize run record {}: {err}",
            record.run_id
        ))
    })?;

    write_new_text_file(&path, &(json + "\n")).map_err(|err| {
        TriadError::Io(format!(
            "failed to write run record {}: {err}",
            path.display()
        ))
    })
}

pub(crate) fn read_run_record(run_dir: &Path, id: &RunId) -> Result<RunRecord, TriadError> {
    read_json_file(&run_json_path(run_dir, id), "run record")
}

pub(crate) fn read_run_records(run_dir: &Path) -> Result<Vec<RunRecord>, TriadError> {
    json_files_in_dir(run_dir)?
        .iter()
        .map(|path| read_json_file(path, "run record"))
        .collect()
}

pub(crate) fn pending_patch_id_for_claim(
    patch_dir: &Path,
    claim_id: &ClaimId,
) -> Result<Option<PatchId>, TriadError> {
    Ok(pending_patch_metas(patch_dir)?
        .into_iter()
        .filter(|meta| &meta.claim_id == claim_id)
        .map(|meta| meta.id)
        .max_by_key(|id| id.sequence_number()))
}

pub(crate) fn pending_patch_ids_by_claim(
    patch_dir: &Path,
) -> Result<HashMap<ClaimId, PatchId>, TriadError> {
    let mut map: HashMap<ClaimId, PatchId> = HashMap::new();
    for meta in pending_patch_metas(patch_dir)? {
        map.entry(meta.claim_id)
            .and_modify(|existing| {
                if meta.id.sequence_number() > existing.sequence_number() {
                    *existing = meta.id.clone();
                }
            })
            .or_insert(meta.id);
    }
    Ok(map)
}

pub(crate) fn latest_pending_patch_id(patch_dir: &Path) -> Result<Option<PatchId>, TriadError> {
    Ok(pending_patch_metas(patch_dir)?
        .into_iter()
        .map(|meta| meta.id)
        .max_by_key(|id| id.sequence_number()))
}

fn pending_patch_metas(patch_dir: &Path) -> Result<Vec<PatchDraftMeta>, TriadError> {
    let mut pending = Vec::new();
    for path in json_files_in_dir(patch_dir)? {
        let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
            TriadError::InvalidState(format!(
                "patch meta path must be valid UTF-8: {}",
                path.display()
            ))
        })?;
        let meta: PatchDraftMeta = read_json_file(&path, "patch meta")?;
        if meta.id.as_str() != stem {
            return Err(TriadError::InvalidState(format!(
                "patch meta id does not match file name: {} != {} in {}",
                meta.id,
                stem,
                path.display()
            )));
        }
        if meta.state == PatchState::Pending {
            pending.push(meta);
        }
    }

    Ok(pending)
}

pub(crate) fn next_patch_id(patch_dir: &Path) -> Result<PatchId, TriadError> {
    let mut max_sequence = 0u32;
    for path in json_files_in_dir(patch_dir)? {
        let stem = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
            TriadError::InvalidState(format!(
                "patch path must be valid UTF-8: {}",
                path.display()
            ))
        })?;
        let patch_id = PatchId::new(stem).map_err(|_| {
            TriadError::InvalidState(format!(
                "patch file name must use PATCH-###### stem: {}",
                path.display()
            ))
        })?;
        max_sequence = max_sequence.max(patch_id.sequence_number());
    }
    PatchId::from_sequence(max_sequence + 1)
}

pub(crate) fn read_patch_meta(
    patch_dir: &Path,
    id: &PatchId,
) -> Result<PatchDraftMeta, TriadError> {
    let json_path = patch_json_path(patch_dir, id);
    read_json_file(&json_path, "patch meta")
}

pub(crate) fn store_patch_draft(
    repo_root: &Path,
    claim_dir: &camino::Utf8Path,
    patch_dir: &Path,
    draft: &PatchDraft,
) -> Result<(), TriadError> {
    let json_path = patch_json_path(patch_dir, &draft.id);
    let diff_path = patch_diff_path(patch_dir, &draft.id);
    let diff_path_relative = repo_relative_utf8(repo_root, &diff_path)?;
    let base_claim_digest = claim_base_digest(repo_root, claim_dir, &draft.claim_id)?;

    if json_path.exists() || diff_path.exists() {
        return Err(TriadError::InvalidState(format!(
            "patch draft already exists for {}",
            draft.id
        )));
    }

    ensure_dir(patch_dir)?;

    let meta = PatchDraftMeta {
        id: draft.id.clone(),
        claim_id: draft.claim_id.clone(),
        based_on_evidence: draft.based_on_evidence.clone(),
        rationale: draft.rationale.clone(),
        created_at: draft.created_at.clone(),
        state: draft.state,
        diff_path: diff_path_relative,
        base_claim_digest,
    };
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|err| {
        TriadError::Serialization(format!(
            "failed to serialize patch meta {}: {err}",
            draft.id
        ))
    })?;

    write_new_text_file(&diff_path, &draft.unified_diff).map_err(|err| {
        TriadError::Io(format!(
            "failed to write patch diff {}: {err}",
            diff_path.display()
        ))
    })?;
    if let Err(err) = write_new_text_file(&json_path, &(meta_json + "\n")) {
        let _ = fs::remove_file(&diff_path);
        return Err(TriadError::Io(format!(
            "failed to write patch meta {}: {err}",
            json_path.display()
        )));
    }

    Ok(())
}

pub(crate) fn read_patch_draft(
    repo_root: &Path,
    patch_dir: &Path,
    id: &PatchId,
) -> Result<PatchDraft, TriadError> {
    let json_path = patch_json_path(patch_dir, id);
    let meta: PatchDraftMeta = read_json_file(&json_path, "patch meta")?;
    if &meta.id != id {
        return Err(TriadError::InvalidState(format!(
            "patch meta id does not match requested patch id: {} != {} in {}",
            meta.id,
            id,
            json_path.display()
        )));
    }
    let expected_diff_path = repo_relative_utf8(repo_root, &patch_diff_path(patch_dir, id))?;
    if meta.diff_path != expected_diff_path {
        return Err(TriadError::InvalidState(format!(
            "patch meta diff path does not match patch id {}: {}",
            id, meta.diff_path
        )));
    }

    let diff_path = resolve_repo_relative_path(repo_root, &meta.diff_path)?;
    let unified_diff = fs::read_to_string(&diff_path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            TriadError::InvalidState(format!(
                "patch diff file is missing for {}: {}",
                id,
                diff_path.display()
            ))
        } else {
            TriadError::Io(format!(
                "failed to read patch diff {}: {err}",
                diff_path.display()
            ))
        }
    })?;

    Ok(PatchDraft {
        id: meta.id,
        claim_id: meta.claim_id,
        based_on_evidence: meta.based_on_evidence,
        unified_diff,
        rationale: meta.rationale,
        created_at: meta.created_at,
        state: meta.state,
    })
}

pub(crate) fn update_patch_draft_state(
    repo_root: &Path,
    patch_dir: &Path,
    id: &PatchId,
    state: PatchState,
) -> Result<(), TriadError> {
    let meta = read_patch_meta(patch_dir, id)?;
    let json_path = patch_json_path(patch_dir, id);
    let diff_path = patch_diff_path(patch_dir, id);
    let diff_path_relative = repo_relative_utf8(repo_root, &diff_path)?;
    let meta = PatchDraftMeta {
        id: meta.id,
        claim_id: meta.claim_id,
        based_on_evidence: meta.based_on_evidence,
        rationale: meta.rationale,
        created_at: meta.created_at,
        state,
        diff_path: diff_path_relative,
        base_claim_digest: meta.base_claim_digest,
    };
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|err| {
        TriadError::Serialization(format!("failed to serialize patch meta {}: {err}", id))
    })?;

    fs::write(&json_path, meta_json + "\n").map_err(|err| {
        TriadError::Io(format!(
            "failed to update patch meta {}: {err}",
            json_path.display()
        ))
    })
}

pub(crate) fn json_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, TriadError> {
    ensure_dir(dir)?;
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| {
        TriadError::Io(format!("failed to read directory {}: {err}", dir.display()))
    })? {
        let entry = entry.map_err(|err| {
            TriadError::Io(format!(
                "failed to read directory entry in {}: {err}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TriadError::Io(format!(
                "failed to read file type for {}: {err}",
                path.display()
            ))
        })?;
        if file_type.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    Ok(paths)
}

pub(crate) fn read_text_file(path: &Path, kind: &str) -> Result<String, TriadError> {
    fs::read_to_string(path)
        .map_err(|err| TriadError::Io(format!("failed to read {kind} {}: {err}", path.display())))
}

pub(crate) fn read_json_file<T>(path: &Path, kind: &str) -> Result<T, TriadError>
where
    T: DeserializeOwned,
{
    let content = read_text_file(path, kind)?;
    serde_json::from_str(&content).map_err(|err| {
        TriadError::Serialization(format!(
            "invalid {kind} {}: {}",
            path.display(),
            normalize_serde_row_error(&err.to_string())
        ))
    })
}

pub(crate) fn write_new_text_file(path: &Path, content: &str) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    use std::io::Write;
    file.write_all(content.as_bytes())
}
