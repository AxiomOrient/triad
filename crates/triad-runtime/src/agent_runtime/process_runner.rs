use std::io::Write;
use std::process::{Command, Stdio};

use triad_core::TriadError;

use super::{PreparedProcessInvocation, ProcessCaptureMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessExecutionOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub(crate) trait ProcessRunner {
    fn run(
        &self,
        invocation: &PreparedProcessInvocation,
    ) -> Result<ProcessExecutionOutput, TriadError>;
}

#[derive(Debug, Default)]
pub(crate) struct HostProcessRunner;

impl ProcessRunner for HostProcessRunner {
    fn run(
        &self,
        invocation: &PreparedProcessInvocation,
    ) -> Result<ProcessExecutionOutput, TriadError> {
        let mut command = Command::new(&invocation.program);
        command
            .args(&invocation.args)
            .current_dir(invocation.cwd.as_std_path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if invocation.stdin.is_some() {
            command.stdin(Stdio::piped());
        }

        for (key, value) in &invocation.env {
            command.env(key, value);
        }

        let _ = invocation.timeout;
        let mut child = command.spawn().map_err(|err| {
            TriadError::Io(format!(
                "failed to spawn runtime command `{}`: {err}",
                invocation.program
            ))
        })?;

        if let Some(stdin) = invocation.stdin.as_ref() {
            let mut handle = child.stdin.take().ok_or_else(|| {
                TriadError::InvalidState(format!(
                    "runtime command `{}` did not expose stdin pipe",
                    invocation.program
                ))
            })?;
            handle.write_all(stdin.as_bytes()).map_err(|err| {
                TriadError::Io(format!(
                    "failed to write stdin for runtime command `{}`: {err}",
                    invocation.program
                ))
            })?;
        }

        let output = child.wait_with_output().map_err(|err| {
            TriadError::Io(format!(
                "failed to wait for runtime command `{}`: {err}",
                invocation.program
            ))
        })?;
        let exit_code = output.status.code().ok_or_else(|| {
            TriadError::RuntimeBlocked(format!(
                "runtime command terminated without exit code: {}",
                invocation.program
            ))
        })?;
        let stderr = String::from_utf8(output.stderr).map_err(|err| {
            TriadError::Serialization(format!(
                "runtime stderr for `{}` was not valid UTF-8: {err}",
                invocation.program
            ))
        })?;
        let stdout = match &invocation.capture_mode {
            ProcessCaptureMode::Stdout => String::from_utf8(output.stdout).map_err(|err| {
                TriadError::Serialization(format!(
                    "runtime stdout for `{}` was not valid UTF-8: {err}",
                    invocation.program
                ))
            })?,
            ProcessCaptureMode::OutputFile { path } => std::fs::read_to_string(path.as_std_path())
                .map_err(|err| {
                    TriadError::Io(format!(
                        "failed to read runtime output file `{}`: {err}",
                        path
                    ))
                })?,
        };

        Ok(ProcessExecutionOutput {
            stdout,
            stderr,
            exit_code,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, time::Duration};

    use camino::Utf8PathBuf;

    use super::{HostProcessRunner, ProcessRunner};
    use crate::agent_runtime::{PreparedProcessInvocation, ProcessCaptureMode};

    #[test]
    fn adapter_contract_host_process_runner_captures_stdout() {
        let temp = tempfile_dir("host-process-runner-stdout");
        let runner = HostProcessRunner;
        let output = runner
            .run(&PreparedProcessInvocation {
                program: "sh".to_string(),
                args: vec!["-lc".to_string(), "printf '{\"ok\":true}'".to_string()],
                env: BTreeMap::new(),
                cwd: Utf8PathBuf::from_path_buf(temp.clone())
                    .expect("temp path should be valid UTF-8"),
                stdin: None,
                timeout: Duration::from_secs(5),
                capture_mode: ProcessCaptureMode::Stdout,
                model: "test-model".to_string(),
                effort: "medium".to_string(),
            })
            .expect("process should run");

        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "{\"ok\":true}");
        assert_eq!(output.stderr, "");
    }

    #[test]
    fn adapter_contract_host_process_runner_reads_output_file() {
        let temp = tempfile_dir("host-process-runner-file");
        let output_path = temp.join("last-message.json");
        let runner = HostProcessRunner;
        let output = runner
            .run(&PreparedProcessInvocation {
                program: "sh".to_string(),
                args: vec![
                    "-lc".to_string(),
                    format!("printf '{{\"ok\":true}}' > {}", output_path.display()),
                ],
                env: BTreeMap::new(),
                cwd: Utf8PathBuf::from_path_buf(temp.clone())
                    .expect("temp path should be valid UTF-8"),
                stdin: None,
                timeout: Duration::from_secs(5),
                capture_mode: ProcessCaptureMode::OutputFile {
                    path: Utf8PathBuf::from_path_buf(output_path.clone())
                        .expect("path should be valid UTF-8"),
                },
                model: "test-model".to_string(),
                effort: "medium".to_string(),
            })
            .expect("process should run");

        assert_eq!(output.exit_code, 0);
        assert_eq!(output.stdout, "{\"ok\":true}");
    }

    fn tempfile_dir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "triad-{}-{}",
            label,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }
}
