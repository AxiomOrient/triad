use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};
use triad_core::{Claim, ClaimId, TriadError};

use crate::LocalTriad;
use crate::repo_support::{
    repo_relative_utf8, resolve_repo_relative_path, sha256_prefixed_hex, utf8_path,
};
use crate::storage::read_text_file;

#[derive(Debug, Clone)]
pub(crate) struct ClaimLoadIssue {
    pub(crate) candidate_id: Option<ClaimId>,
    pub(crate) diagnostic: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedClaimCatalog {
    pub(crate) claims: Vec<Claim>,
    pub(crate) issues: Vec<ClaimLoadIssue>,
}

pub(crate) fn parsed_claim_by_id_or_issue(
    triad: &LocalTriad,
    id: &ClaimId,
) -> Result<Claim, TriadError> {
    let catalog = triad.parsed_claim_catalog()?;
    if let Some(claim) = catalog.claims.iter().find(|claim| &claim.id == id) {
        return Ok(claim.clone());
    }
    if let Some(issue) = claim_issue_for_id(&catalog.issues, id) {
        return Err(TriadError::InvalidState(issue.diagnostic.clone()));
    }
    Err(TriadError::InvalidState(format!("claim not found: {}", id)))
}

pub(crate) fn parse_claim_catalog(
    paths: Vec<Utf8PathBuf>,
) -> Result<ParsedClaimCatalog, TriadError> {
    let mut catalog = ParsedClaimCatalog::default();

    for path in paths {
        match parse_claim_file(&path) {
            Ok(claim) => catalog.claims.push(claim),
            Err(err) => catalog.issues.push(claim_load_issue(&path, err)),
        }
    }

    Ok(catalog)
}

pub(crate) fn claim_issue_for_id<'a>(
    issues: &'a [ClaimLoadIssue],
    id: &ClaimId,
) -> Option<&'a ClaimLoadIssue> {
    issues.iter().find(|issue| {
        issue
            .candidate_id
            .as_ref()
            .is_some_and(|candidate| candidate == id)
    })
}

pub(crate) fn no_valid_claims_error(issues: &[ClaimLoadIssue]) -> TriadError {
    if let Some(issue) = issues.first() {
        TriadError::InvalidState(format!("no valid claims available; {}", issue.diagnostic))
    } else {
        TriadError::InvalidState("no claims available for next".into())
    }
}

pub(crate) fn claim_base_digest(
    repo_root: &Path,
    claim_dir: &Utf8Path,
    claim_id: &ClaimId,
) -> Result<Option<String>, TriadError> {
    let claim_path = claim_md_path(claim_dir, claim_id);
    let claim_path = resolve_repo_relative_path(
        repo_root,
        &repo_relative_utf8(repo_root, claim_path.as_std_path())?,
    )?;
    if !claim_path.is_file() {
        return Ok(None);
    }

    let claim = parse_claim_file(&utf8_path(claim_path, "claim path")?)?;
    Ok(Some(claim_revision_digest(&claim)))
}

pub(crate) fn claim_revision_number(claim: &Claim) -> u32 {
    let digest = Sha256::digest(canonical_claim_revision_bytes(claim));
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

pub(crate) fn discover_claim_file_paths(claim_dir: &Path) -> Result<Vec<Utf8PathBuf>, TriadError> {
    if !claim_dir.exists() {
        return Err(TriadError::InvalidState(format!(
            "claim directory does not exist: {}",
            claim_dir.display()
        )));
    }

    if !claim_dir.is_dir() {
        return Err(TriadError::InvalidState(format!(
            "claim directory is not a directory: {}",
            claim_dir.display()
        )));
    }

    let mut claim_files = Vec::new();

    for entry in std::fs::read_dir(claim_dir).map_err(|err| {
        TriadError::Io(format!(
            "failed to read claim directory {}: {err}",
            claim_dir.display()
        ))
    })? {
        let entry = entry.map_err(|err| {
            TriadError::Io(format!(
                "failed to read claim directory entry in {}: {err}",
                claim_dir.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TriadError::Io(format!(
                "failed to read file type for {}: {err}",
                path.display()
            ))
        })?;

        if file_type.is_dir() {
            return Err(TriadError::InvalidState(format!(
                "nested claim directory is not allowed: {}",
                path.display()
            )));
        }

        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        claim_files.push(utf8_path(path, "claim path")?);
    }

    claim_files.sort();
    Ok(claim_files)
}

pub(crate) fn claim_md_path(claim_dir: &Utf8Path, id: &ClaimId) -> Utf8PathBuf {
    claim_dir.join(format!("{}.md", id.as_str()))
}

pub(crate) fn parse_claim_file(path: &Utf8Path) -> Result<Claim, TriadError> {
    let content = read_text_file(path.as_std_path(), "claim file")?;
    let lines: Vec<&str> = content.lines().collect();
    let first_line = lines
        .first()
        .ok_or_else(|| TriadError::Parse(format!("claim file is missing H1: {}", path)))?;
    let (h1_id, title) = parse_claim_h1(first_line, path)?;
    let file_name_id = path.file_stem().ok_or_else(|| {
        TriadError::Parse(format!("claim file is missing a valid stem: {}", path))
    })?;

    if file_name_id == h1_id.as_str() {
        let sections = parse_claim_sections(&lines, path)?;
        let claim = Claim {
            id: h1_id,
            title,
            statement: section_text(&sections.claim),
            examples: bullet_items(&sections.examples, "Examples", sections.examples_line, path)?,
            invariants: bullet_items(
                &sections.invariants,
                "Invariants",
                sections.invariants_line,
                path,
            )?,
            notes: sections.notes.map(|body| section_text(&body)),
            revision: 0,
        };
        let mut claim = claim;
        claim.revision = claim_revision_number(&claim);
        canonical_claim_revision_bytes(&claim);

        return Ok(claim);
    }

    Err(TriadError::Parse(format!(
        "claim file name does not match H1 id: {} != {} in {}",
        file_name_id,
        h1_id.as_str(),
        path
    )))
}

pub(crate) fn canonical_claim_revision_bytes(claim: &Claim) -> Vec<u8> {
    let mut lines = Vec::new();

    lines.push(format!(
        "# {} {}",
        claim.id.as_str(),
        normalize_inline_text(&claim.title)
    ));
    lines.push(String::new());
    lines.push("## Claim".to_string());
    lines.extend(normalize_multiline_text(&claim.statement));
    lines.push(String::new());
    lines.push("## Examples".to_string());
    lines.extend(
        claim
            .examples
            .iter()
            .map(|item| format!("- {}", normalize_inline_text(item))),
    );
    lines.push(String::new());
    lines.push("## Invariants".to_string());
    lines.extend(
        claim
            .invariants
            .iter()
            .map(|item| format!("- {}", normalize_inline_text(item))),
    );

    if let Some(notes) = claim.notes.as_ref() {
        lines.push(String::new());
        lines.push("## Notes".to_string());
        lines.extend(normalize_multiline_text(notes));
    }

    let mut canonical = lines.join("\n");
    canonical.push('\n');
    canonical.into_bytes()
}

pub(crate) fn canonical_claim_lines(claim: &Claim) -> Vec<String> {
    String::from_utf8(canonical_claim_revision_bytes(claim))
        .expect("canonical claim bytes should remain UTF-8")
        .lines()
        .map(str::to_string)
        .collect()
}

fn claim_load_issue(path: &Utf8Path, error: TriadError) -> ClaimLoadIssue {
    let candidate_id = path.file_stem().and_then(|stem| ClaimId::new(stem).ok());
    let diagnostic = if let Some(id) = candidate_id.as_ref() {
        format!("malformed claim {id}: {error}")
    } else {
        format!("malformed claim file {path}: {error}")
    };

    ClaimLoadIssue {
        candidate_id,
        diagnostic,
    }
}

pub(crate) fn claim_revision_digest(claim: &Claim) -> String {
    sha256_prefixed_hex(&Sha256::digest(canonical_claim_revision_bytes(claim)))
}

fn parse_claim_h1(first_line: &str, path: &Utf8Path) -> Result<(ClaimId, String), TriadError> {
    let h1 = first_line
        .strip_prefix("# ")
        .ok_or_else(|| TriadError::Parse(format!("claim file is missing H1: {}", path)))?;
    let (claim_id, title) = h1
        .split_once(' ')
        .ok_or_else(|| TriadError::Parse(format!("claim file H1 is missing title: {}", path)))?;
    if title.trim().is_empty() {
        return Err(TriadError::Parse(format!(
            "claim file H1 is missing title: {}",
            path
        )));
    }

    let claim_id = ClaimId::new(claim_id).map_err(|_| {
        TriadError::Parse(format!(
            "claim file H1 has invalid claim id {} in {}",
            claim_id, path
        ))
    })?;

    Ok((claim_id, title.to_string()))
}

struct ParsedSections<'a> {
    claim: Vec<&'a str>,
    examples: Vec<&'a str>,
    examples_line: usize,
    invariants: Vec<&'a str>,
    invariants_line: usize,
    notes: Option<Vec<&'a str>>,
}

fn parse_claim_sections<'a>(
    lines: &[&'a str],
    path: &Utf8Path,
) -> Result<ParsedSections<'a>, TriadError> {
    let mut sections: Vec<(&str, usize, Vec<&str>)> = Vec::new();
    let mut current_heading: Option<(&str, usize, Vec<&str>)> = None;

    for (index, line) in lines.iter().enumerate().skip(1) {
        if let Some(heading) = line.strip_prefix("### ") {
            return Err(TriadError::Parse(format!(
                "unexpected subsection {} at line {} in {}",
                heading.trim(),
                index + 1,
                path
            )));
        }

        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(section) = current_heading.take() {
                sections.push(section);
            }
            current_heading = Some((heading.trim(), index + 1, Vec::new()));
            continue;
        }

        if let Some((_, _, body)) = current_heading.as_mut() {
            body.push(*line);
        } else if !line.trim().is_empty() {
            return Err(TriadError::Parse(format!(
                "unexpected content before first section at line {} in {}",
                index + 1,
                path
            )));
        }
    }

    if let Some(section) = current_heading.take() {
        sections.push(section);
    }

    let expected = ["Claim", "Examples", "Invariants"];
    let mut expected_index = 0usize;
    let mut notes_seen = false;

    let mut claim = None;
    let mut examples = None;
    let mut invariants = None;
    let mut notes = None;

    for (heading, line_no, body) in sections {
        if expected_index < expected.len() && heading == expected[expected_index] {
            validate_section_body(heading, &body, line_no, path)?;
            match heading {
                "Claim" => claim = Some((body, line_no)),
                "Examples" => examples = Some((body, line_no)),
                "Invariants" => invariants = Some((body, line_no)),
                _ => {}
            }
            expected_index += 1;
            continue;
        }

        if heading == "Notes" && expected_index == expected.len() && !notes_seen {
            validate_section_body(heading, &body, line_no, path)?;
            notes = Some(body);
            notes_seen = true;
            continue;
        }

        if expected[expected_index..].contains(&heading) || heading == "Notes" {
            let expected_heading = expected
                .get(expected_index)
                .copied()
                .unwrap_or("end of file");
            return Err(TriadError::Parse(format!(
                "section {} is out of order, expected {} at line {} in {}",
                heading, expected_heading, line_no, path
            )));
        }

        return Err(TriadError::Parse(format!(
            "unexpected section {} at line {} in {}",
            heading, line_no, path
        )));
    }

    if expected_index < expected.len() {
        return Err(TriadError::Parse(format!(
            "missing required section {} in {}",
            expected[expected_index], path
        )));
    }

    let (claim, _) = claim.expect("validated claim section should exist");
    let (examples, examples_line) = examples.expect("validated examples section should exist");
    let (invariants, invariants_line) =
        invariants.expect("validated invariants section should exist");

    Ok(ParsedSections {
        claim,
        examples,
        examples_line,
        invariants,
        invariants_line,
        notes,
    })
}

fn validate_section_body(
    heading: &str,
    body: &[&str],
    line_no: usize,
    path: &Utf8Path,
) -> Result<(), TriadError> {
    match heading {
        "Claim" => {
            if body.iter().all(|line| line.trim().is_empty()) {
                return Err(TriadError::Parse(format!(
                    "section Claim must contain body text at line {} in {}",
                    line_no, path
                )));
            }
        }
        "Examples" | "Invariants" => {
            let mut bullet_count = 0usize;

            for (offset, line) in body.iter().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }

                let trimmed = line.trim_start();
                let Some(item) = trimmed.strip_prefix("- ") else {
                    return Err(TriadError::Parse(format!(
                        "section {} must contain only bullet items at line {} in {}",
                        heading,
                        line_no + offset + 1,
                        path
                    )));
                };

                if item.trim().is_empty() {
                    return Err(TriadError::Parse(format!(
                        "section {} contains an empty bullet at line {} in {}",
                        heading,
                        line_no + offset + 1,
                        path
                    )));
                }

                bullet_count += 1;
            }

            if bullet_count == 0 {
                return Err(TriadError::Parse(format!(
                    "section {} must contain at least one bullet in {}",
                    heading, path
                )));
            }
        }
        "Notes" => {}
        _ => {}
    }

    Ok(())
}

fn section_text(lines: &[&str]) -> String {
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(lines.len());

    lines[start..end].join("\n")
}

fn bullet_items(
    lines: &[&str],
    heading: &str,
    line_no: usize,
    path: &Utf8Path,
) -> Result<Vec<String>, TriadError> {
    let mut items = Vec::new();

    for (offset, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        let trimmed = line.trim_start();
        let item = trimmed.strip_prefix("- ").ok_or_else(|| {
            TriadError::Parse(format!(
                "section {} must contain only bullet items at line {} in {}",
                heading,
                line_no + offset + 1,
                path
            ))
        })?;
        if item.trim().is_empty() {
            return Err(TriadError::Parse(format!(
                "section {} contains an empty bullet at line {} in {}",
                heading,
                line_no + offset + 1,
                path
            )));
        }
        items.push(item.to_string());
    }

    Ok(items)
}

fn normalize_inline_text(value: &str) -> String {
    value.trim().to_string()
}

fn normalize_multiline_text(value: &str) -> Vec<String> {
    let normalized: Vec<String> = value
        .split('\n')
        .map(|line| line.trim_end_matches([' ', '\t', '\r']).to_string())
        .collect();

    let start = normalized
        .iter()
        .position(|line| !line.is_empty())
        .unwrap_or(0);
    let end = normalized
        .iter()
        .rposition(|line| !line.is_empty())
        .map(|index| index + 1)
        .unwrap_or(normalized.len());

    normalized[start..end].to_vec()
}
