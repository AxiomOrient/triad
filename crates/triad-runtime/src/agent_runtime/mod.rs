pub(crate) mod adapter;
pub(crate) mod backend_probe;
pub(crate) mod claude;
pub(crate) mod codex;
pub(crate) mod gemini;
pub(crate) mod process_runner;
pub(crate) mod session;
pub(crate) mod workspace_stage;

pub(crate) use adapter::{
    AdapterRunRequest, AgentRuntimeAdapter, PreparedProcessInvocation, ProcessCaptureMode,
    RawInvocationOutput,
};
pub(crate) use backend_probe::{
    BackendCapabilityProbe, probe_backend_capabilities, validate_run_request_against_probe,
};
pub(crate) use claude::ClaudeAdapter;
pub(crate) use codex::CodexAdapter;
pub(crate) use gemini::GeminiAdapter;
pub(crate) use process_runner::{HostProcessRunner, ProcessRunner};
pub(crate) use session::{
    ApprovalPolicy, PromptAttachment, ReasoningEffort, RunProfile, SandboxPolicy, SandboxPreset,
    SessionConfig,
};
pub(crate) use workspace_stage::{WorkspaceChangeKind, WorkspaceStage, stage_workspace};
