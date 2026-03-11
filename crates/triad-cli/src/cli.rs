use clap::{Args, Parser, Subcommand, ValueEnum};
use triad_core::ReasoningLevel;

#[derive(Debug, Parser)]
#[command(name = "triad", version, about = "Claim/evidence ratchet CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Next,
    Work(WorkArgs),
    Verify(VerifyArgs),
    Accept(AcceptArgs),
    Status(StatusArgs),
    Agent(AgentArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct WorkArgs {
    pub claim_id: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long, value_enum)]
    pub effort: Option<Effort>,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    pub claim_id: Option<String>,
    #[arg(long)]
    pub with_probe: bool,
    #[arg(long)]
    pub full_workspace: bool,
}

#[derive(Debug, Args)]
pub struct AcceptArgs {
    #[arg(required_unless_present = "latest")]
    pub patch_id: Option<String>,
    #[arg(long, conflicts_with = "patch_id")]
    pub latest: bool,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub claim: Option<String>,
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    Claim(AgentClaimArgs),
    Drift(AgentDriftArgs),
    Run(AgentRunArgs),
    Verify(AgentVerifyArgs),
    Patch(AgentPatchArgs),
    Status(AgentStatusArgs),
}

#[derive(Debug, Args)]
pub struct AgentClaimArgs {
    #[command(subcommand)]
    pub command: AgentClaimCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentClaimCommand {
    List,
    Get { claim_id: String },
    Next,
}

#[derive(Debug, Args)]
pub struct AgentDriftArgs {
    #[command(subcommand)]
    pub command: AgentDriftCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentDriftCommand {
    Detect {
        #[arg(long)]
        claim: String,
    },
}

#[derive(Debug, Args)]
pub struct AgentRunArgs {
    #[arg(long)]
    pub claim: String,
}

#[derive(Debug, Args)]
pub struct AgentVerifyArgs {
    #[arg(long)]
    pub claim: String,
    #[arg(long)]
    pub with_probe: bool,
    #[arg(long)]
    pub full_workspace: bool,
}

#[derive(Debug, Args)]
pub struct AgentPatchArgs {
    #[command(subcommand)]
    pub command: AgentPatchCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentPatchCommand {
    Propose {
        #[arg(long)]
        claim: String,
    },
    Apply {
        #[arg(long)]
        patch: String,
    },
}

#[derive(Debug, Args)]
pub struct AgentStatusArgs {
    #[arg(long)]
    pub claim: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl From<Effort> for ReasoningLevel {
    fn from(value: Effort) -> Self {
        match value {
            Effort::Low => Self::Low,
            Effort::Medium => Self::Medium,
            Effort::High => Self::High,
        }
    }
}
