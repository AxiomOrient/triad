use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "triad",
    version,
    about = "Headless deterministic verification kernel CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init(InitArgs),
    Lint(LintArgs),
    Verify(VerifyArgs),
    Report(ReportArgs),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct LintArgs {
    #[arg(long, conflicts_with = "all")]
    pub claim: Option<String>,
    #[arg(long, conflicts_with = "claim")]
    pub all: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct VerifyArgs {
    #[arg(long)]
    pub claim: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    #[arg(long, conflicts_with = "all")]
    pub claim: Option<String>,
    #[arg(long, conflicts_with = "claim")]
    pub all: bool,
    #[arg(long)]
    pub json: bool,
}
