use camino::Utf8PathBuf;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "triad",
    version,
    about = "Headless deterministic verification kernel CLI"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Use PATH as the triad repo root instead of discovering from the working directory"
    )]
    pub repo_root: Option<Utf8PathBuf>,
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
