use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

use camino::Utf8Path;
use fs2::FileExt;
use serde::de::DeserializeOwned;
use triad_core::{Evidence, EvidenceId, TriadError};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EvidenceNdjsonStore;

impl EvidenceNdjsonStore {
    pub fn next_evidence_id(path: &Utf8Path) -> Result<EvidenceId, TriadError> {
        if !path.exists() {
            return EvidenceId::from_sequence(1);
        }

        let content = fs::read_to_string(path)
            .map_err(|err| TriadError::Io(format!("failed to read evidence log {path}: {err}")))?;
        Self::next_evidence_id_from_content(&content)
    }

    pub fn append(path: &Utf8Path, evidence: &Evidence) -> Result<(), TriadError> {
        let evidence = evidence.clone();
        Self::append_new(path, move |_| Ok(evidence)).map(|_| ())
    }

    pub fn append_new<F>(path: &Utf8Path, build: F) -> Result<Evidence, TriadError>
    where
        F: FnOnce(EvidenceId) -> Result<Evidence, TriadError>,
    {
        ensure_parent_dir(path)?;
        let mut file = open_evidence_log(path)?;
        file.lock_exclusive()
            .map_err(|err| TriadError::Io(format!("failed to lock evidence log {path}: {err}")))?;

        let append_result = Self::append_new_locked(path, &mut file, build);
        drop(file);
        append_result
    }

    pub fn read(path: &Utf8Path) -> Result<Vec<Evidence>, TriadError> {
        Self::read_rows(path, "evidence row")
    }

    fn read_rows<T>(path: &Utf8Path, row_kind: &str) -> Result<Vec<T>, TriadError>
    where
        T: DeserializeOwned,
    {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(path)
            .map_err(|err| TriadError::Io(format!("failed to read evidence log {path}: {err}")))?;
        Self::read_rows_from_str(&content, row_kind, path)
    }

    fn read_rows_from_str<T>(
        content: &str,
        row_kind: &str,
        path: &Utf8Path,
    ) -> Result<Vec<T>, TriadError>
    where
        T: DeserializeOwned,
    {
        content
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| {
                serde_json::from_str(line).map_err(|err| {
                    TriadError::Serialization(format!(
                        "invalid {row_kind} at line {} in {}: {}",
                        index + 1,
                        path,
                        normalize_serde_row_error(&err.to_string())
                    ))
                })
            })
            .collect()
    }

    fn append_new_locked<F>(
        path: &Utf8Path,
        file: &mut File,
        build: F,
    ) -> Result<Evidence, TriadError>
    where
        F: FnOnce(EvidenceId) -> Result<Evidence, TriadError>,
    {
        let content = read_locked_content(path, file)?;
        if !content.is_empty() && !content.ends_with('\n') {
            return Err(TriadError::InvalidState(format!(
                "evidence log must end with newline before append: {path}"
            )));
        }

        let expected_id = Self::next_evidence_id_from_content(&content)?;
        let evidence = build(expected_id.clone())?;
        if evidence.id != expected_id {
            return Err(TriadError::InvalidState(format!(
                "evidence id must be next monotonic id for {path}: expected {expected_id}, got {}",
                evidence.id
            )));
        }

        let serialized = serde_json::to_string(&evidence).map_err(|err| {
            TriadError::Serialization(format!(
                "failed to serialize evidence {}: {err}",
                evidence.id
            ))
        })?;
        writeln!(file, "{serialized}")
            .map_err(|err| TriadError::Io(format!("failed to append evidence to {path}: {err}")))?;
        Ok(evidence)
    }

    fn next_evidence_id_from_content(content: &str) -> Result<EvidenceId, TriadError> {
        let rows = Self::read_rows_from_str::<EvidenceIdRow>(
            content,
            "evidence row",
            Utf8Path::new("<memory>"),
        )?;
        let max_sequence = rows
            .into_iter()
            .map(|row| row.id.sequence_number())
            .max()
            .unwrap_or(0);
        EvidenceId::from_sequence(max_sequence + 1)
    }
}

#[derive(serde::Deserialize)]
struct EvidenceIdRow {
    id: EvidenceId,
}

fn open_evidence_log(path: &Utf8Path) -> Result<File, TriadError> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .map_err(|err| TriadError::Io(format!("failed to open evidence log {path}: {err}")))
}

fn ensure_parent_dir(path: &Utf8Path) -> Result<(), TriadError> {
    let parent = path
        .parent()
        .ok_or_else(|| TriadError::InvalidState(format!("path has no parent directory: {path}")))?;
    fs::create_dir_all(parent)
        .map_err(|err| TriadError::Io(format!("failed to create directory {parent}: {err}")))
}

fn read_locked_content(path: &Utf8Path, file: &mut File) -> Result<String, TriadError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|err| TriadError::Io(format!("failed to seek evidence log {path}: {err}")))?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|err| TriadError::Io(format!("failed to read evidence log {path}: {err}")))?;
    Ok(content)
}

fn normalize_serde_row_error(message: &str) -> &str {
    message
        .split_once(" at line ")
        .map(|(head, _)| head)
        .unwrap_or(message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::process;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use camino::Utf8PathBuf;
    use triad_core::{
        ClaimId, Evidence, EvidenceClass, EvidenceId, EvidenceKind, Provenance, Verdict,
    };

    use super::EvidenceNdjsonStore;

    fn temp_file(label: &str) -> Utf8PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "triad-fs-evidence-{label}-{}-{unique}",
            process::id()
        ));
        fs::create_dir_all(&root).expect("temp dir should create");
        Utf8PathBuf::from_path_buf(root.join("evidence.ndjson")).expect("utf8 temp path")
    }

    fn evidence(id: u32) -> Evidence {
        Evidence {
            id: EvidenceId::from_sequence(id).expect("evidence id should format"),
            claim_id: ClaimId::new("REQ-auth-001").expect("claim id should parse"),
            class: EvidenceClass::Hard,
            kind: EvidenceKind::Test,
            verdict: Verdict::Pass,
            verifier: "cargo test".into(),
            claim_revision_digest: "sha256:claim".into(),
            artifact_digests: BTreeMap::from([("src/auth.rs".into(), "sha256:file".into())]),
            command: Some("cargo test".into()),
            locator: Some("auth::login_success".into()),
            summary: Some("fresh pass".into()),
            provenance: Provenance {
                actor: "ci".into(),
                runtime: Some("cargo-test".into()),
                session_id: None,
                task_id: None,
                workflow_id: None,
                commit: None,
                environment_digest: None,
            },
            created_at: "2026-03-12T00:00:00Z".into(),
        }
    }

    #[test]
    fn append_and_read_roundtrip_preserves_order() {
        let path = temp_file("roundtrip");
        let first = evidence(1);
        let second = evidence(2);

        EvidenceNdjsonStore::append(&path, &first).expect("first append should succeed");
        EvidenceNdjsonStore::append(&path, &second).expect("second append should succeed");

        let rows = EvidenceNdjsonStore::read(&path).expect("rows should read");
        assert_eq!(rows, vec![first, second]);
    }

    #[test]
    fn read_reports_malformed_line() {
        let path = temp_file("malformed");
        let valid = serde_json::to_string(&evidence(1)).expect("evidence should serialize");
        fs::write(&path, format!("{valid}\nnot-json\n")).expect("evidence file should write");

        let error = EvidenceNdjsonStore::read(&path).expect_err("malformed row should fail");
        assert!(error.to_string().contains("invalid evidence row at line 2"));
    }

    #[test]
    fn append_new_serializes_concurrent_writers() {
        let path = temp_file("concurrent");
        let start = Arc::new(Barrier::new(2));

        let handles = (0..2)
            .map(|_| {
                let path = path.clone();
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    start.wait();
                    EvidenceNdjsonStore::append_new(&path, |id| {
                        thread::sleep(Duration::from_millis(25));
                        Ok(evidence(id.sequence_number()))
                    })
                    .expect("append should succeed")
                })
            })
            .collect::<Vec<_>>();

        let mut appended_ids = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread should complete").id)
            .collect::<Vec<_>>();
        appended_ids.sort();

        assert_eq!(
            appended_ids,
            vec![
                EvidenceId::from_sequence(1).expect("evidence id should format"),
                EvidenceId::from_sequence(2).expect("evidence id should format"),
            ]
        );

        let stored_ids = EvidenceNdjsonStore::read(&path)
            .expect("rows should read")
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>();
        assert_eq!(
            stored_ids,
            vec![
                EvidenceId::from_sequence(1).expect("evidence id should format"),
                EvidenceId::from_sequence(2).expect("evidence id should format"),
            ]
        );
    }
}
