use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use triad_core::{ClaimId, TriadError};

use crate::repo_support::utf8_path;
use crate::{LocalTriad, parsed_claim_by_id_or_issue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkGuardrails {
    repo_root: Utf8PathBuf,
    temp_workspace_root: Utf8PathBuf,
    allowed_write_roots: Vec<Utf8PathBuf>,
    forbid_direct_spec_edits: bool,
    forbid_git_commit: bool,
    forbid_git_push: bool,
    forbid_destructive_rm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkToolUse {
    Exec { program: String, args: Vec<String> },
    WritePath { path: Utf8PathBuf },
    RemovePath { path: Utf8PathBuf, recursive: bool },
}

pub(crate) fn build_work_guardrails(
    triad: &LocalTriad,
    claim_id: &ClaimId,
    allowed_write_roots: &[Utf8PathBuf],
) -> Result<WorkGuardrails, TriadError> {
    let _claim = parsed_claim_by_id_or_issue(triad, claim_id)?;
    let allowed_write_roots = allowed_write_roots
        .iter()
        .map(|path| normalize_guardrail_path(&triad.config.repo_root, path))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(WorkGuardrails {
        repo_root: triad.config.repo_root.clone(),
        temp_workspace_root: triad.config.paths.state_dir.join("tmp"),
        allowed_write_roots,
        forbid_direct_spec_edits: triad.config.guardrails.forbid_direct_spec_edits,
        forbid_git_commit: triad.config.guardrails.forbid_git_commit,
        forbid_git_push: triad.config.guardrails.forbid_git_push,
        forbid_destructive_rm: triad.config.guardrails.forbid_destructive_rm,
    })
}

impl WorkGuardrails {
    pub fn check(&self, tool_use: &WorkToolUse) -> Result<(), TriadError> {
        match tool_use {
            WorkToolUse::Exec { program, args } => self.check_exec(program, args),
            WorkToolUse::WritePath { path } => self.check_write_path(path),
            WorkToolUse::RemovePath { path, recursive } => self.check_remove_path(path, *recursive),
        }
    }

    fn check_exec(&self, program: &str, args: &[String]) -> Result<(), TriadError> {
        if program == "git"
            && self.forbid_git_commit
            && args.first().is_some_and(|arg| arg == "commit")
        {
            return Err(TriadError::RuntimeBlocked(
                "git commit blocked by work guardrails".to_string(),
            ));
        }

        if program == "git" && self.forbid_git_push && args.first().is_some_and(|arg| arg == "push")
        {
            return Err(TriadError::RuntimeBlocked(
                "git push blocked by work guardrails".to_string(),
            ));
        }

        Ok(())
    }

    fn check_write_path(&self, path: &Utf8Path) -> Result<(), TriadError> {
        let path = normalize_guardrail_path(&self.repo_root, path)?;
        self.check_protected_spec_path(&path)?;
        self.check_allowed_write_root(&path)
    }

    fn check_remove_path(&self, path: &Utf8Path, recursive: bool) -> Result<(), TriadError> {
        let path = normalize_guardrail_path(&self.repo_root, path)?;
        self.check_protected_spec_path(&path)?;

        if recursive && self.forbid_destructive_rm && !path.starts_with(&self.temp_workspace_root) {
            return Err(TriadError::RuntimeBlocked(format!(
                "destructive recursive remove blocked outside temp workspace: {}",
                display_guardrail_path(&self.repo_root, &path)
            )));
        }

        self.check_allowed_write_root(&path)
    }

    fn check_protected_spec_path(&self, path: &Utf8Path) -> Result<(), TriadError> {
        let spec_root = self.repo_root.join("spec/claims");
        if self.forbid_direct_spec_edits && path.starts_with(&spec_root) {
            return Err(TriadError::RuntimeBlocked(format!(
                "direct spec edit blocked: {}",
                display_guardrail_path(&self.repo_root, path)
            )));
        }

        Ok(())
    }

    fn check_allowed_write_root(&self, path: &Utf8Path) -> Result<(), TriadError> {
        if self
            .allowed_write_roots
            .iter()
            .any(|root| path.starts_with(root))
        {
            return Ok(());
        }

        Err(TriadError::RuntimeBlocked(format!(
            "unrelated write blocked: {}",
            display_guardrail_path(&self.repo_root, path)
        )))
    }
}

fn normalize_guardrail_path(
    repo_root: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, TriadError> {
    let absolute = if path.is_absolute() {
        path.as_std_path().to_path_buf()
    } else {
        repo_root.join(path).into_std_path_buf()
    };

    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }

    let normalized = utf8_path(normalized, "guardrail path")?;
    if !normalized.starts_with(repo_root) {
        return Err(TriadError::RuntimeBlocked(format!(
            "path escaped repo root: {}",
            normalized
        )));
    }

    Ok(normalized)
}

fn display_guardrail_path(repo_root: &Utf8Path, path: &Utf8Path) -> String {
    path.strip_prefix(repo_root)
        .map(|relative| relative.as_str().to_string())
        .unwrap_or_else(|_| path.as_str().to_string())
}
