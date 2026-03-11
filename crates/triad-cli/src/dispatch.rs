use std::io::Write;

use anyhow::{Result, bail};
use triad_core::RunClaimRequest;

use crate::CliRuntime;
use crate::agent_output::{agent_claim_get_data, agent_claim_list_data, write_agent_envelope};
use crate::cli::{
    AcceptArgs, AgentClaimCommand, AgentCommand, AgentDriftCommand, AgentPatchCommand, Command,
    Effort, StatusArgs, VerifyArgs, WorkArgs,
};
use crate::exit_codes::{
    CliExit, exit_code_for_accept, exit_code_for_claim_summaries, exit_code_for_drift,
    exit_code_for_next, exit_code_for_status, exit_code_for_verify, exit_code_for_work,
};
use crate::human_output;
use crate::parsing::{
    parse_claim_id, parse_optional_claim_id, parse_patch_id, resolve_accept_patch_id,
    resolve_claim_id, resolve_verify_request,
};

pub(crate) fn dispatch_command<R: CliRuntime>(
    runtime: &R,
    command: Command,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    match command {
        Command::Next => dispatch_human_next(runtime, stdout),
        Command::Work(args) => dispatch_human_work(runtime, args, stdout),
        Command::Verify(args) => dispatch_human_verify(runtime, args, stdout),
        Command::Accept(args) => dispatch_human_accept(runtime, args, stdout),
        Command::Status(args) => dispatch_human_status(runtime, args, stdout),
        Command::Agent(agent) => dispatch_agent_command(runtime, agent.command, stdout),
        Command::Init(_) => bail!("init must be handled before runtime dispatch"),
    }
}

fn dispatch_human_next<R: CliRuntime>(runtime: &R, stdout: &mut impl Write) -> Result<CliExit> {
    let next = runtime.next_claim()?;
    human_output::write_next(stdout, &next, &runtime.claim_load_diagnostics()?)?;
    Ok(exit_code_for_next(&next))
}

fn dispatch_human_work<R: CliRuntime>(
    runtime: &R,
    args: WorkArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let claim_id = resolve_claim_id(runtime, args.claim_id)?;
    let report = runtime.run_claim(RunClaimRequest {
        claim_id,
        dry_run: args.dry_run,
        model: args.model,
        effort: args.effort.map(Effort::into),
    })?;
    human_output::write_work(stdout, &report)?;
    Ok(exit_code_for_work(&report))
}

fn dispatch_human_verify<R: CliRuntime>(
    runtime: &R,
    args: VerifyArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let report = runtime.verify_claim(resolve_verify_request(
        runtime,
        args.claim_id,
        args.with_probe,
        args.full_workspace,
    )?)?;
    human_output::write_verify(stdout, &report)?;
    Ok(exit_code_for_verify(&report))
}

fn dispatch_human_accept<R: CliRuntime>(
    runtime: &R,
    args: AcceptArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let patch_id = resolve_accept_patch_id(runtime, args)?;
    let report = runtime.apply_patch(&patch_id)?;
    human_output::write_accept(stdout, &report)?;
    Ok(exit_code_for_accept(&report))
}

fn dispatch_human_status<R: CliRuntime>(
    runtime: &R,
    args: StatusArgs,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    let claim_id = parse_optional_claim_id(args.claim.as_deref())?;
    let report = runtime.status(claim_id.as_ref())?;
    human_output::write_status(
        stdout,
        &report,
        claim_id.as_ref(),
        args.verbose,
        &runtime.claim_load_diagnostics()?,
    )?;
    Ok(exit_code_for_status(&report))
}

fn dispatch_agent_command<R: CliRuntime>(
    runtime: &R,
    command: AgentCommand,
    stdout: &mut impl Write,
) -> Result<CliExit> {
    match command {
        AgentCommand::Claim(args) => match args.command {
            AgentClaimCommand::List => {
                let claims = runtime.list_claims()?;
                let data = agent_claim_list_data(&claims);
                write_agent_envelope(stdout, "claim.list", &data)?;
                Ok(exit_code_for_claim_summaries(&claims))
            }
            AgentClaimCommand::Get { claim_id } => {
                let bundle = runtime.get_claim(&parse_claim_id(&claim_id)?)?;
                let data = agent_claim_get_data(&bundle);
                write_agent_envelope(stdout, "claim.get", &data).map(|_| CliExit::Success)
            }
            AgentClaimCommand::Next => {
                let next = runtime.next_claim()?;
                write_agent_envelope(stdout, "claim.next", &next)?;
                Ok(exit_code_for_next(&next))
            }
        },
        AgentCommand::Drift(args) => match args.command {
            AgentDriftCommand::Detect { claim } => {
                let drift = runtime.detect_drift(&parse_claim_id(&claim)?)?;
                write_agent_envelope(stdout, "drift.detect", &drift)?;
                Ok(exit_code_for_drift(&drift))
            }
        },
        AgentCommand::Run(args) => {
            let report = runtime.run_claim(RunClaimRequest {
                claim_id: parse_claim_id(&args.claim)?,
                dry_run: false,
                model: None,
                effort: None,
            })?;
            write_agent_envelope(stdout, "run", &report)?;
            Ok(exit_code_for_work(&report))
        }
        AgentCommand::Verify(args) => {
            let report = runtime.verify_claim(runtime.default_verify_request(
                parse_claim_id(&args.claim)?,
                args.with_probe,
                args.full_workspace,
            )?)?;
            write_agent_envelope(stdout, "verify", &report)?;
            Ok(exit_code_for_verify(&report))
        }
        AgentCommand::Patch(args) => match args.command {
            AgentPatchCommand::Propose { claim } => {
                let report = runtime.propose_patch(&parse_claim_id(&claim)?)?;
                write_agent_envelope(stdout, "patch.propose", &report)?;
                Ok(CliExit::PatchApprovalRequired)
            }
            AgentPatchCommand::Apply { patch } => {
                let report = runtime.apply_patch(&parse_patch_id(&patch)?)?;
                write_agent_envelope(stdout, "patch.apply", &report)?;
                Ok(exit_code_for_accept(&report))
            }
        },
        AgentCommand::Status(args) => {
            let claim_id = parse_optional_claim_id(args.claim.as_deref())?;
            let report = runtime.status(claim_id.as_ref())?;
            write_agent_envelope(stdout, "status", &report)?;
            Ok(exit_code_for_status(&report))
        }
    }
}
