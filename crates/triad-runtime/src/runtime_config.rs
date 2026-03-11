use std::str::FromStr;

use camino::Utf8Path;
use triad_config::CanonicalTriadConfig;
use triad_core::TriadError;

use crate::agent_runtime::{
    ApprovalPolicy, ReasoningEffort, RunProfile, SandboxPolicy, SandboxPreset, SessionConfig,
};

pub(crate) fn run_profile_from_triad(
    config: &CanonicalTriadConfig,
) -> Result<RunProfile, TriadError> {
    let effort = ReasoningEffort::from_str(&config.agent.effort)
        .map_err(|detail| TriadError::config_field("agent.effort", &detail))?;
    let approval_policy = ApprovalPolicy::from_str(&config.agent.approval_policy)
        .map_err(|detail| TriadError::config_field("agent.approval_policy", &detail))?;
    let sandbox_policy = sandbox_policy_from_triad(config, &config.repo_root)?;
    let timeout = std::time::Duration::from_secs(config.agent.timeout_seconds);

    if timeout.is_zero() {
        return Err(TriadError::config_field(
            "agent.timeout_seconds",
            "must be > 0",
        ));
    }

    Ok(RunProfile::new()
        .with_model(config.agent.model.clone())
        .with_effort(effort)
        .with_approval_policy(approval_policy)
        .with_sandbox_policy(sandbox_policy)
        .with_timeout(timeout))
}

pub(crate) fn session_config_from_triad(
    config: &CanonicalTriadConfig,
) -> Result<SessionConfig, TriadError> {
    session_config_from_triad_in_root(config, &config.repo_root)
}

pub(crate) fn session_config_from_triad_in_root(
    config: &CanonicalTriadConfig,
    cwd: &Utf8Path,
) -> Result<SessionConfig, TriadError> {
    let mut profile = run_profile_from_triad(config)?;
    profile.sandbox_policy = sandbox_policy_from_triad(config, cwd)?;
    Ok(SessionConfig::from_profile(cwd.as_str(), profile))
}

fn sandbox_policy_from_triad(
    config: &CanonicalTriadConfig,
    cwd: &Utf8Path,
) -> Result<SandboxPolicy, TriadError> {
    let cwd = cwd.as_str().to_string();

    match config.agent.sandbox_policy.as_str() {
        "read-only" => Ok(SandboxPolicy::Preset(SandboxPreset::ReadOnly)),
        "workspace-write" => Ok(SandboxPolicy::Preset(SandboxPreset::WorkspaceWrite {
            writable_roots: vec![cwd],
            network_access: false,
        })),
        "danger-full-access" => Ok(SandboxPolicy::Preset(SandboxPreset::DangerFullAccess)),
        other => Err(TriadError::config_field(
            "agent.sandbox_policy",
            &format!("unknown sandbox policy: {other}"),
        )),
    }
}
