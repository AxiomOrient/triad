mod cli;
mod dispatch;
mod exit_codes;
mod output;
mod parsing;

use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use cli::{Cli, Command};
use dispatch::{dispatch_command, dispatch_init};
use exit_codes::{CliExit, exit_code_for_error};
use triad_fs::{CONFIG_FILE_NAME, TriadConfig, init_scaffold};

fn main() -> ExitCode {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    finalize_cli_result(execute_cli(Cli::parse(), &mut stdout), &mut stderr).as_exit_code()
}

fn execute_cli(cli: Cli, stdout: &mut impl Write) -> Result<CliExit> {
    let working_dir = env::current_dir().context("failed to resolve current directory")?;
    let working_dir = Utf8PathBuf::from_path_buf(working_dir).map_err(|path| {
        anyhow::anyhow!("working directory is not valid UTF-8: {}", path.display())
    })?;
    execute_cli_from_dir(cli, stdout, &working_dir)
}

pub(crate) fn execute_cli_from_dir(
    cli: Cli,
    stdout: &mut impl Write,
    working_dir: &Utf8PathBuf,
) -> Result<CliExit> {
    match cli.command {
        Command::Init(args) => {
            let repo_root = discover_repo_root(working_dir).unwrap_or_else(|| working_dir.clone());
            init_scaffold(&repo_root, args.force).context("failed to create triad scaffold")?;
            dispatch_init(&repo_root, stdout)
        }
        command => {
            let repo_root = discover_repo_root(working_dir)
                .ok_or_else(|| anyhow::anyhow!("failed to discover triad repo root"))?;
            let config = load_config(&repo_root)?;
            dispatch_command(&config, command, stdout)
        }
    }
}

fn finalize_cli_result(result: Result<CliExit>, stderr: &mut impl Write) -> CliExit {
    match result {
        Ok(exit) => exit,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            exit_code_for_error(&error)
        }
    }
}

fn discover_repo_root(start: &Utf8PathBuf) -> Option<Utf8PathBuf> {
    start
        .ancestors()
        .find(|path| path.join(CONFIG_FILE_NAME).is_file())
        .map(|path| path.to_owned())
}

fn load_config(repo_root: &Utf8PathBuf) -> Result<triad_fs::CanonicalTriadConfig> {
    TriadConfig::from_file(repo_root.join(CONFIG_FILE_NAME))
        .context("failed to load triad.toml")?
        .canonicalize(repo_root)
        .context("failed to canonicalize triad config")
}

#[cfg(test)]
mod tests;
