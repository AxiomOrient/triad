use triad_config::CanonicalTriadConfig;
use triad_core::{ClaimId, TriadError, VerifyLayer, VerifyRequest};

use crate::repo_support::unique_non_empty_strings;
use crate::{LocalTriad, VerifyCommandPlan, read_run_records};

pub(crate) fn resolve_targeted_selectors(
    triad: &LocalTriad,
    claim_id: &ClaimId,
) -> Result<Vec<String>, TriadError> {
    let latest_run = read_run_records(triad.config.paths.run_dir.as_std_path())?
        .into_iter()
        .filter(|r| &r.claim_id == claim_id && !r.suggested_test_selectors.is_empty())
        .max_by_key(|r| r.run_id.sequence_number());

    if let Some(record) = latest_run {
        return Ok(unique_non_empty_strings(record.suggested_test_selectors));
    }

    let relevant = triad.relevant_evidence_for_claim(claim_id)?;
    let mut evidence = [relevant.pass, relevant.fail, relevant.unknown]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| right.id.sequence_number().cmp(&left.id.sequence_number()));

    Ok(unique_non_empty_strings(
        evidence
            .into_iter()
            .filter_map(|evidence| evidence.test_selector),
    ))
}

pub(crate) fn plan_verify_commands(
    triad: &LocalTriad,
    req: &VerifyRequest,
) -> Result<Vec<VerifyCommandPlan>, TriadError> {
    let selectors = if req.full_workspace {
        Vec::new()
    } else {
        triad.resolve_targeted_selectors(&req.claim_id)?
    };
    let targeted = !selectors.is_empty();
    let mut plans = Vec::new();

    for layer in req.layers.iter().copied() {
        if targeted {
            for selector in &selectors {
                plans.push(VerifyCommandPlan {
                    layer,
                    command: targeted_verify_command(layer, selector),
                    targeted: true,
                });
            }
        } else {
            plans.push(VerifyCommandPlan {
                layer,
                command: workspace_verify_command(layer),
                targeted: false,
            });
        }
    }

    Ok(plans)
}

pub(crate) fn default_verify_request(
    config: &CanonicalTriadConfig,
    claim_id: ClaimId,
    with_probe: bool,
    full_workspace: bool,
) -> Result<VerifyRequest, TriadError> {
    let mut layers = configured_default_verify_layers(config)?;
    if with_probe {
        layers.push(VerifyLayer::Probe);
    }

    Ok(VerifyRequest {
        claim_id,
        layers,
        full_workspace,
    })
}

fn configured_default_verify_layers(
    config: &CanonicalTriadConfig,
) -> Result<Vec<VerifyLayer>, TriadError> {
    let mut layers = Vec::with_capacity(config.verify.default_layers.len());

    for raw_layer in &config.verify.default_layers {
        let layer = parse_verify_layer_name(raw_layer)?;
        if layer == VerifyLayer::Probe {
            return Err(TriadError::config_field(
                "verify.default_layers",
                "probe must be enabled only via --with-probe",
            ));
        }
        layers.push(layer);
    }

    if layers.is_empty() {
        return Err(TriadError::config_field(
            "verify.default_layers",
            "must include at least one default layer",
        ));
    }

    Ok(layers)
}

fn parse_verify_layer_name(raw_layer: &str) -> Result<VerifyLayer, TriadError> {
    match raw_layer {
        "unit" => Ok(VerifyLayer::Unit),
        "contract" => Ok(VerifyLayer::Contract),
        "integration" => Ok(VerifyLayer::Integration),
        "probe" => Ok(VerifyLayer::Probe),
        _ => Err(TriadError::config_field(
            "verify.default_layers",
            &format!("unknown layer `{raw_layer}`"),
        )),
    }
}

fn targeted_verify_command(layer: VerifyLayer, selector: &str) -> String {
    match layer {
        VerifyLayer::Unit => format!("cargo test --lib {selector}"),
        VerifyLayer::Contract => format!("cargo test {selector}"),
        VerifyLayer::Integration => format!("cargo test --tests {selector}"),
        VerifyLayer::Probe => format!("cargo test --tests {selector} -- --ignored"),
    }
}

fn workspace_verify_command(layer: VerifyLayer) -> String {
    match layer {
        VerifyLayer::Unit => "cargo test --workspace --lib".to_string(),
        VerifyLayer::Contract => "cargo test --workspace".to_string(),
        VerifyLayer::Integration => "cargo test --workspace --tests".to_string(),
        VerifyLayer::Probe => "cargo test --workspace --tests -- --ignored".to_string(),
    }
}
