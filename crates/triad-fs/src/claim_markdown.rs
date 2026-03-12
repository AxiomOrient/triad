use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{Claim, ClaimId, TriadError, canonical_claim_text, compute_claim_revision_digest};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClaimMarkdownAdapter;

impl ClaimMarkdownAdapter {
    pub fn discover_claim_file_paths(claim_dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>, TriadError> {
        if !claim_dir.exists() {
            return Err(TriadError::InvalidState(format!(
                "claim directory does not exist: {claim_dir}"
            )));
        }
        if !claim_dir.is_dir() {
            return Err(TriadError::InvalidState(format!(
                "claim directory is not a directory: {claim_dir}"
            )));
        }

        let mut claim_files = Vec::new();
        for entry in fs::read_dir(claim_dir).map_err(|err| {
            TriadError::Io(format!("failed to read claim directory {claim_dir}: {err}"))
        })? {
            let entry = entry.map_err(|err| {
                TriadError::Io(format!(
                    "failed to read claim directory entry in {claim_dir}: {err}"
                ))
            })?;
            let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                TriadError::InvalidState(format!(
                    "claim path is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
            let file_type = entry.file_type().map_err(|err| {
                TriadError::Io(format!("failed to read file type for {path}: {err}"))
            })?;

            if file_type.is_dir() {
                return Err(TriadError::InvalidState(format!(
                    "nested claim directory is not allowed: {path}"
                )));
            }
            if file_type.is_file() && path.extension() == Some("md") {
                claim_files.push(path);
            }
        }

        claim_files.sort();
        Ok(claim_files)
    }

    pub fn parse_claim_file(path: &Utf8Path) -> Result<Claim, TriadError> {
        let content = fs::read_to_string(path)
            .map_err(|err| TriadError::Io(format!("failed to read claim file {path}: {err}")))?;
        Self::parse_claim_str(&content, path)
    }

    pub fn parse_claim_str(input: &str, path: &Utf8Path) -> Result<Claim, TriadError> {
        let lines: Vec<&str> = input.lines().collect();
        let first_line = lines
            .first()
            .ok_or_else(|| TriadError::Parse(format!("claim file is missing H1: {path}")))?;
        let (claim_id, title) = parse_claim_h1(first_line, path)?;
        let file_stem = path.file_stem().ok_or_else(|| {
            TriadError::Parse(format!("claim file is missing a valid stem: {path}"))
        })?;

        if file_stem != claim_id.as_str() {
            return Err(TriadError::Parse(format!(
                "claim file name does not match H1 id: {} != {} in {}",
                file_stem,
                claim_id.as_str(),
                path
            )));
        }

        let sections = parse_sections(&lines[1..], path)?;
        let mut claim = Claim {
            id: claim_id,
            title,
            statement: join_section_text(&sections.claim),
            examples: parse_bullets(&sections.examples, "Examples", path)?,
            invariants: parse_bullets(&sections.invariants, "Invariants", path)?,
            notes: sections.notes.map(|notes| join_section_text(&notes)),
            revision_digest: String::new(),
        };
        claim.revision_digest = compute_claim_revision_digest(&claim);
        Ok(claim)
    }

    pub fn write_claim_file(path: &Utf8Path, claim: &Claim) -> Result<(), TriadError> {
        let contents = canonical_claim_text(claim);
        fs::write(path, contents)
            .map_err(|err| TriadError::Io(format!("failed to write claim file {path}: {err}")))
    }
}

#[derive(Debug)]
struct ParsedSections {
    claim: Vec<String>,
    examples: Vec<String>,
    invariants: Vec<String>,
    notes: Option<Vec<String>>,
}

fn parse_claim_h1(line: &str, path: &Utf8Path) -> Result<(ClaimId, String), TriadError> {
    let rest = line
        .strip_prefix("# ")
        .ok_or_else(|| TriadError::Parse(format!("claim file H1 must start with `# `: {path}")))?;
    let (id, title) = rest.split_once(' ').ok_or_else(|| {
        TriadError::Parse(format!("claim file H1 must contain `<ID> <Title>`: {path}"))
    })?;

    Ok((ClaimId::new(id)?, title.trim().to_string()))
}

fn parse_sections(lines: &[&str], path: &Utf8Path) -> Result<ParsedSections, TriadError> {
    let mut index = 0;
    let claim = read_section(lines, &mut index, "Claim", path)?;
    let examples = read_section(lines, &mut index, "Examples", path)?;
    let invariants = read_section(lines, &mut index, "Invariants", path)?;
    let notes = if index < lines.len() {
        Some(read_section(lines, &mut index, "Notes", path)?)
    } else {
        None
    };

    while index < lines.len() && lines[index].trim().is_empty() {
        index += 1;
    }
    if index != lines.len() {
        return Err(TriadError::Parse(format!(
            "unexpected extra content after claim sections in {path}"
        )));
    }

    Ok(ParsedSections {
        claim,
        examples,
        invariants,
        notes,
    })
}

fn read_section(
    lines: &[&str],
    index: &mut usize,
    expected_heading: &str,
    path: &Utf8Path,
) -> Result<Vec<String>, TriadError> {
    while *index < lines.len() && lines[*index].trim().is_empty() {
        *index += 1;
    }

    let expected = format!("## {expected_heading}");
    let line = lines.get(*index).ok_or_else(|| {
        TriadError::Parse(format!("missing `## {expected_heading}` section in {path}"))
    })?;
    if line.trim() != expected {
        return Err(TriadError::Parse(format!(
            "expected `## {expected_heading}` section in {path}, got `{}`",
            line.trim()
        )));
    }
    *index += 1;

    let mut body = Vec::new();
    while *index < lines.len() && !lines[*index].starts_with("## ") {
        body.push(lines[*index].to_string());
        *index += 1;
    }

    Ok(trim_blank_lines(body))
}

fn trim_blank_lines(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines
}

fn join_section_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn parse_bullets(
    lines: &[String],
    section: &str,
    path: &Utf8Path,
) -> Result<Vec<String>, TriadError> {
    if lines.is_empty() {
        return Err(TriadError::Parse(format!(
            "`## {section}` must contain at least one bullet in {path}"
        )));
    }

    lines
        .iter()
        .enumerate()
        .map(|(offset, line)| {
            let item = line.strip_prefix("- ").ok_or_else(|| {
                TriadError::Parse(format!(
                    "`## {section}` line {} must start with `- ` in {}",
                    offset + 1,
                    path
                ))
            })?;
            let trimmed = item.trim();
            if trimmed.is_empty() {
                Err(TriadError::Parse(format!(
                    "`## {section}` line {} must not be empty in {}",
                    offset + 1,
                    path
                )))
            } else {
                Ok(trimmed.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use triad_core::compute_claim_revision_digest;

    use super::ClaimMarkdownAdapter;

    fn temp_dir(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "triad-fs-claims-{label}-{}-{unique}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(path).expect("utf8 temp path")
    }

    #[test]
    fn parse_claim_file_roundtrips_into_revision_digest() {
        let repo_root = temp_dir("parse");
        let path = repo_root.join("REQ-auth-001.md");
        fs::write(
            &path,
            "# REQ-auth-001 Login success\n\n## Claim\nSystem grants access with valid credentials.\n\n## Examples\n- valid -> 200\n- invalid -> 401\n\n## Invariants\n- session issued\n\n## Notes\nMFA out of scope.\n",
        )
        .expect("claim file should write");

        let claim = ClaimMarkdownAdapter::parse_claim_file(&path).expect("claim should parse");

        assert_eq!(claim.id.as_str(), "REQ-auth-001");
        assert_eq!(claim.revision_digest, compute_claim_revision_digest(&claim));

        let rewritten = repo_root.join("REQ-auth-001.rewrite.md");
        ClaimMarkdownAdapter::write_claim_file(&rewritten, &claim).expect("claim should write");
        let rewritten_text = fs::read_to_string(&rewritten).expect("rewritten claim should read");
        assert!(rewritten_text.contains("## Examples\n- valid -> 200\n- invalid -> 401"));
    }

    #[test]
    fn discover_claim_file_paths_sorts_and_filters_markdown_files() {
        let repo_root = temp_dir("discover");
        fs::write(repo_root.join("REQ-b-002.md"), "").expect("claim should write");
        fs::write(repo_root.join("REQ-a-001.md"), "").expect("claim should write");
        fs::write(repo_root.join("notes.txt"), "").expect("note should write");

        let paths = ClaimMarkdownAdapter::discover_claim_file_paths(&repo_root)
            .expect("discovery should succeed");

        assert_eq!(
            paths
                .iter()
                .map(|path| path.file_name().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["REQ-a-001.md", "REQ-b-002.md"]
        );
    }
}
