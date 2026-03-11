mod agent_output;
mod cli;
mod dispatch;
mod exit_codes;
mod human_output;
mod parsing;

use std::{
    env,
    io::{self, Write},
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command};
use triad_config::{CONFIG_FILE_NAME, CanonicalTriadConfig, TriadConfig};
use triad_core::{ClaimId, PatchId, TriadApi, TriadError, VerifyRequest};
use triad_runtime::LocalTriad;

use dispatch::dispatch_command;
use exit_codes::{CliExit, exit_code_for_error};

trait CliRuntime: TriadApi {
    fn default_verify_request(
        &self,
        claim_id: ClaimId,
        with_probe: bool,
        full_workspace: bool,
    ) -> Result<VerifyRequest, TriadError>;

    fn claim_load_diagnostics(&self) -> Result<Vec<String>, TriadError> {
        Ok(Vec::new())
    }

    fn latest_pending_patch_id(&self) -> Result<Option<PatchId>, TriadError> {
        Ok(self
            .status(None)?
            .claims
            .into_iter()
            .filter_map(|claim| claim.pending_patch_id)
            .max_by_key(|id| id.sequence_number()))
    }
}

impl CliRuntime for LocalTriad {
    fn default_verify_request(
        &self,
        claim_id: ClaimId,
        with_probe: bool,
        full_workspace: bool,
    ) -> Result<VerifyRequest, TriadError> {
        LocalTriad::default_verify_request(self, claim_id, with_probe, full_workspace)
    }

    fn claim_load_diagnostics(&self) -> Result<Vec<String>, TriadError> {
        LocalTriad::claim_load_diagnostics(self)
    }

    fn latest_pending_patch_id(&self) -> Result<Option<PatchId>, TriadError> {
        LocalTriad::latest_pending_patch_id(self)
    }
}

fn main() -> ExitCode {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    finalize_cli_result(execute_cli(Cli::parse(), &mut stdout), &mut stderr).as_exit_code()
}

fn execute_cli(cli: Cli, stdout: &mut impl Write) -> Result<CliExit> {
    let working_dir = env::current_dir().context("failed to resolve current directory")?;
    execute_cli_from_dir(cli, stdout, &working_dir)
}

fn execute_cli_from_dir(
    cli: Cli,
    stdout: &mut impl Write,
    working_dir: &std::path::Path,
) -> Result<CliExit> {
    match cli.command {
        Command::Init(args) => {
            load_runtime_for_init(working_dir)?
                .init_scaffold(args.force)
                .context("failed to create triad scaffold")?;

            Ok(CliExit::Success)
        }
        command => dispatch_command(&load_runtime_from_dir(working_dir)?, command, stdout),
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

fn load_runtime_from_dir(working_dir: &std::path::Path) -> Result<LocalTriad> {
    let repo_root = LocalTriad::discover_repo_root(working_dir)
        .context("failed to discover triad repo root")?;
    let config = load_config(&repo_root)?;

    Ok(LocalTriad::new(config))
}

fn load_runtime_for_init(working_dir: &std::path::Path) -> Result<LocalTriad> {
    let repo_root =
        LocalTriad::discover_repo_root(working_dir).unwrap_or_else(|_| working_dir.to_path_buf());
    let config = load_init_config(&repo_root)?;

    Ok(LocalTriad::new(config))
}

fn load_config(repo_root: &std::path::Path) -> Result<CanonicalTriadConfig> {
    TriadConfig::from_repo_root(repo_root)
        .context("failed to load triad.toml")?
        .canonicalize(repo_root)
        .context("failed to canonicalize triad config")?
        .validate()
        .context("failed to validate triad config")
}

fn load_init_config(repo_root: &std::path::Path) -> Result<CanonicalTriadConfig> {
    let config = if repo_root.join(CONFIG_FILE_NAME).is_file() {
        TriadConfig::from_repo_root(repo_root).context("failed to load triad.toml")?
    } else {
        TriadConfig::bootstrap_defaults()
    };

    config
        .canonicalize(repo_root)
        .context("failed to canonicalize triad config")?
        .validate_for_init()
        .context("failed to validate triad config for init")
}

#[cfg(test)]
mod tests;
