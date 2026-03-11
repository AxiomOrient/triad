use std::{str::FromStr, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl FromStr for ReasoningEffort {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!("unknown reasoning effort: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalPolicy {
    Never,
}

impl FromStr for ApprovalPolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "never" => Ok(Self::Never),
            other => Err(format!("unknown approval policy: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxPreset {
    ReadOnly,
    WorkspaceWrite {
        writable_roots: Vec<String>,
        network_access: bool,
    },
    DangerFullAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxPolicy {
    Preset(SandboxPreset),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAttachment {
    AtPath {
        path: String,
        placeholder: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunProfile {
    pub model: Option<String>,
    pub effort: ReasoningEffort,
    pub approval_policy: ApprovalPolicy,
    pub sandbox_policy: SandboxPolicy,
    pub timeout: Duration,
}

impl RunProfile {
    pub fn new() -> Self {
        Self {
            model: None,
            effort: ReasoningEffort::Medium,
            approval_policy: ApprovalPolicy::Never,
            sandbox_policy: SandboxPolicy::Preset(SandboxPreset::ReadOnly),
            timeout: Duration::from_secs(0),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_effort(mut self, effort: ReasoningEffort) -> Self {
        self.effort = effort;
        self
    }

    pub fn with_approval_policy(mut self, approval_policy: ApprovalPolicy) -> Self {
        self.approval_policy = approval_policy;
        self
    }

    pub fn with_sandbox_policy(mut self, sandbox_policy: SandboxPolicy) -> Self {
        self.sandbox_policy = sandbox_policy;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionConfig {
    pub cwd: String,
    pub model: Option<String>,
    pub effort: ReasoningEffort,
    pub approval_policy: ApprovalPolicy,
    pub sandbox_policy: SandboxPolicy,
    pub timeout: Duration,
    pub attachments: Vec<PromptAttachment>,
    pub output_schema: Option<serde_json::Value>,
}

impl SessionConfig {
    pub fn from_profile(cwd: &str, profile: RunProfile) -> Self {
        Self {
            cwd: cwd.to_string(),
            model: profile.model,
            effort: profile.effort,
            approval_policy: profile.approval_policy,
            sandbox_policy: profile.sandbox_policy,
            timeout: profile.timeout,
            attachments: Vec::new(),
            output_schema: None,
        }
    }
}
