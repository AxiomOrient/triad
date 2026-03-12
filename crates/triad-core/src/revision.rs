use sha2::{Digest, Sha256};

use crate::Claim;

pub fn canonical_claim_text(claim: &Claim) -> String {
    let mut sections = vec![
        format!(
            "# {} {}",
            claim.id.as_str(),
            normalize_inline_text(&claim.title)
        ),
        format!("## Claim\n{}", normalize_block_text(&claim.statement)),
        format!("## Examples\n{}", canonical_bullets(&claim.examples)),
        format!("## Invariants\n{}", canonical_bullets(&claim.invariants)),
    ];

    if let Some(notes) = claim.notes.as_deref() {
        let notes = normalize_block_text(notes);
        if !notes.is_empty() {
            sections.push(format!("## Notes\n{notes}"));
        }
    }

    let mut canonical = sections.join("\n\n");
    canonical.push('\n');
    canonical
}

pub fn compute_claim_revision_digest(claim: &Claim) -> String {
    let digest = Sha256::digest(canonical_claim_text(claim).as_bytes());
    format!("sha256:{digest:x}")
}

fn canonical_bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {}", normalize_inline_text(item)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_inline_text(value: &str) -> String {
    normalize_block_text(value)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_block_text(value: &str) -> String {
    let mut lines = value
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect::<Vec<_>>();

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use crate::{Claim, ClaimId};

    use super::{canonical_claim_text, compute_claim_revision_digest};

    fn sample_claim(
        statement: &str,
        examples: Vec<&str>,
        invariants: Vec<&str>,
        notes: Option<&str>,
    ) -> Claim {
        Claim {
            id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            title: "Login success".into(),
            statement: statement.into(),
            examples: examples.into_iter().map(str::to_string).collect(),
            invariants: invariants.into_iter().map(str::to_string).collect(),
            notes: notes.map(str::to_string),
            revision_digest: String::new(),
        }
    }

    #[test]
    fn canonical_claim_text_uses_fixed_markdown_layout() {
        let claim = sample_claim(
            "\nSystem grants access with valid credentials.  \n",
            vec![" valid credentials -> 200  ", " wrong password -> 401 "],
            vec![" session is issued  "],
            Some("\nMFA handled elsewhere.  \n"),
        );

        assert_eq!(
            canonical_claim_text(&claim),
            "# REQ-auth-001 Login success\n\n## Claim\nSystem grants access with valid credentials.\n\n## Examples\n- valid credentials -> 200\n- wrong password -> 401\n\n## Invariants\n- session is issued\n\n## Notes\nMFA handled elsewhere.\n"
        );
    }

    #[test]
    fn compute_claim_revision_digest_ignores_whitespace_noise() {
        let canonical = sample_claim(
            "Line one\n\nLine two",
            vec!["valid -> 200", "invalid -> 401"],
            vec!["no plaintext password"],
            Some("Notes stay here."),
        );
        let noisy = sample_claim(
            "\nLine one  \n\nLine two   \n",
            vec![" valid -> 200  ", "invalid -> 401 "],
            vec![" no plaintext password  "],
            Some("\nNotes stay here.   \n"),
        );

        assert_eq!(
            compute_claim_revision_digest(&canonical),
            compute_claim_revision_digest(&noisy)
        );
    }
}
