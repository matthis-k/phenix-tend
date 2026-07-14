use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use globset::{GlobBuilder, GlobSetBuilder};
use walkdir::{DirEntry, WalkDir};

use crate::model::{Plan, PlanItem, TaskKind, TaskResult, TaskStatus};

const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 30 * 60;
const COMMAND_TIMEOUT_ENV: &str = "TEND_TIMEOUT_SECONDS";
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub fn execute(plan: &Plan) -> Vec<TaskResult> {
    let mut results = Vec::with_capacity(plan.items.len());
    let mut status_by_task = HashMap::new();

    for item in &plan.items {
        if item
            .requires
            .iter()
            .any(|dependency| status_by_task.get(dependency) != Some(&TaskStatus::Passed))
        {
            status_by_task.insert(item.task_id.clone(), TaskStatus::Skipped);
            results.push(TaskResult {
                task_id: item.task_id.clone(),
                status: TaskStatus::Skipped,
                stdout: String::new(),
                stderr: "skipped because a prerequisite did not pass".to_string(),
            });
            continue;
        }

        let result = execute_item(item);
        status_by_task.insert(item.task_id.clone(), result.status);
        results.push(result);
    }

    results
}

pub fn has_failures(results: &[TaskResult]) -> bool {
    results
        .iter()
        .any(|result| result.status == TaskStatus::Failed)
}

fn execute_item(item: &PlanItem) -> TaskResult {
    let result = match &item.kind {
        TaskKind::Command {
            command,
            expect_status,
        } => execute_command(command, *expect_status, &item.workdir, &item.env),
        TaskKind::FilesExist { paths } => execute_files_exist(paths, &item.workdir, true),
        TaskKind::FilesAbsent { paths } => execute_files_exist(paths, &item.workdir, false),
        TaskKind::ForbidText { paths, patterns } => {
            execute_text(paths, patterns, &item.workdir, false)
        }
        TaskKind::RequireText { paths, patterns } => {
            execute_text(paths, patterns, &item.workdir, true)
        }
    };

    match result {
        Ok(output) => TaskResult {
            task_id: item.task_id.clone(),
            status: TaskStatus::Passed,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        Err(output) => TaskResult {
            task_id: item.task_id.clone(),
            status: TaskStatus::Failed,
            stdout: output.stdout,
            stderr: output.stderr,
        },
    }
}

struct CapturedOutput {
    stdout: String,
    stderr: String,
}

fn execute_command(
    command: &[String],
    expect_status: i32,
    workdir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<CapturedOutput, CapturedOutput> {
    let Some((program, arguments)) = command.split_first() else {
        return Err(CapturedOutput {
            stdout: String::new(),
            stderr: "empty command".to_string(),
        });
    };
    let timeout = match command_timeout(env) {
        Ok(timeout) => timeout,
        Err(message) => {
            return Err(CapturedOutput {
                stdout: String::new(),
                stderr: message,
            });
        }
    };

    let mut child = match Command::new(program)
        .args(arguments)
        .current_dir(workdir)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return Err(CapturedOutput {
                stdout: String::new(),
                stderr: format!("spawn '{program}': {error}"),
            });
        }
    };

    let stdout_reader = child.stdout.take().map(read_stream);
    let stderr_reader = child.stderr.take().map(read_stream);
    let (status, timed_out) = match wait_for_child(&mut child, timeout) {
        Ok(result) => result,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CapturedOutput {
                stdout: join_stream(stdout_reader),
                stderr: append_message(join_stream(stderr_reader), format!("wait '{program}': {error}")),
            });
        }
    };

    let stdout = join_stream(stdout_reader);
    let mut stderr = join_stream(stderr_reader);
    if timed_out {
        let seconds = timeout.map_or(0, |duration| duration.as_secs());
        stderr = append_message(
            stderr,
            format!("command timed out after {seconds} second(s): {}", command.join(" ")),
        );
    }

    captured_command_result(status, stdout, stderr, expect_status, timed_out)
}

fn command_timeout(env: &BTreeMap<String, String>) -> Result<Option<Duration>, String> {
    let Some(raw) = env.get(COMMAND_TIMEOUT_ENV) else {
        return Ok(Some(Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECONDS)));
    };
    let seconds = raw.parse::<u64>().map_err(|error| {
        format!("invalid {COMMAND_TIMEOUT_ENV} value '{raw}': expected a non-negative integer: {error}")
    })?;
    Ok((seconds != 0).then(|| Duration::from_secs(seconds)))
}

fn wait_for_child(
    child: &mut Child,
    timeout: Option<Duration>,
) -> std::io::Result<(ExitStatus, bool)> {
    let Some(timeout) = timeout else {
        return child.wait().map(|status| (status, false));
    };
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status, false));
        }
        if Instant::now() >= deadline {
            child.kill()?;
            return child.wait().map(|status| (status, true));
        }
        thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

fn read_stream(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        bytes
    })
}

fn join_stream(reader: Option<thread::JoinHandle<Vec<u8>>>) -> String {
    let bytes = reader
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn append_message(mut output: String, message: String) -> String {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&message);
    output
}

fn captured_command_result(
    status: ExitStatus,
    stdout: String,
    stderr: String,
    expect_status: i32,
    timed_out: bool,
) -> Result<CapturedOutput, CapturedOutput> {
    let captured = CapturedOutput { stdout, stderr };
    if !timed_out && status.code() == Some(expect_status) {
        Ok(captured)
    } else {
        Err(captured)
    }
}

fn execute_files_exist(
    paths: &[String],
    workdir: &Path,
    should_exist: bool,
) -> Result<CapturedOutput, CapturedOutput> {
    let mismatches: Vec<String> = paths
        .iter()
        .filter(|path| resolve_path(path, workdir).exists() != should_exist)
        .cloned()
        .collect();
    if mismatches.is_empty() {
        Ok(empty_output())
    } else {
        Err(CapturedOutput {
            stdout: String::new(),
            stderr: format!("path expectation failed: {}", mismatches.join(", ")),
        })
    }
}

fn execute_text(
    paths: &[String],
    patterns: &[String],
    workdir: &Path,
    require: bool,
) -> Result<CapturedOutput, CapturedOutput> {
    let files = expand_paths(paths, workdir);
    let failures: Vec<String> = patterns
        .iter()
        .filter(|pattern| {
            let found = files.iter().any(|path| {
                std::fs::read_to_string(path)
                    .map(|content| content.contains(pattern.as_str()))
                    .unwrap_or(false)
            });
            found != require
        })
        .cloned()
        .collect();

    if failures.is_empty() {
        Ok(empty_output())
    } else {
        Err(CapturedOutput {
            stdout: String::new(),
            stderr: format!("text expectation failed: {}", failures.join(", ")),
        })
    }
}

fn resolve_path(path: &str, workdir: &Path) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workdir.join(path)
    }
}

fn expand_paths(patterns: &[String], root: &Path) -> Vec<PathBuf> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
            builder.add(glob);
        }
    }
    let Ok(globs) = builder.build() else {
        return Vec::new();
    };

    WalkDir::new(root)
        .into_iter()
        .filter_entry(is_relevant_entry)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            globs.is_match(relative).then(|| entry.path().to_path_buf())
        })
        .collect()
}

fn is_relevant_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    !matches!(
        entry.file_name().to_str(),
        Some(".git" | ".direnv" | "node_modules" | "result" | "target")
    )
}

fn empty_output() -> CapturedOutput {
    CapturedOutput {
        stdout: String::new(),
        stderr: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Phase, PlanReason, Selection};

    fn item(id: &str, requires: &[&str], kind: TaskKind) -> PlanItem {
        PlanItem {
            task_id: id.to_string(),
            description: String::new(),
            phase: Phase::Verify,
            implementation: "default".to_string(),
            kind,
            workdir: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            requires: requires.iter().map(|value| (*value).to_string()).collect(),
            reason: PlanReason::Full,
            matched_files: Vec::new(),
        }
    }

    #[test]
    fn failed_dependencies_skip_consumers() {
        let plan = Plan {
            profile: "verify".to_string(),
            context: "local".to_string(),
            selection: Selection::Full,
            files: Vec::new(),
            items: vec![
                item(
                    "missing",
                    &[],
                    TaskKind::FilesExist {
                        paths: vec!["tend-definitely-missing".to_string()],
                    },
                ),
                item(
                    "consumer",
                    &["missing"],
                    TaskKind::FilesAbsent {
                        paths: vec!["also-missing".to_string()],
                    },
                ),
            ],
        };
        let results = execute(&plan);
        assert_eq!(results[0].status, TaskStatus::Failed);
        assert_eq!(results[1].status, TaskStatus::Skipped);
    }

    #[cfg(unix)]
    #[test]
    fn command_timeout_fails_the_task() {
        let mut command = item(
            "slow",
            &[],
            TaskKind::Command {
                command: vec!["sh".to_string(), "-c".to_string(), "sleep 2".to_string()],
                expect_status: 0,
            },
        );
        command
            .env
            .insert(COMMAND_TIMEOUT_ENV.to_string(), "1".to_string());
        let plan = Plan {
            profile: "verify".to_string(),
            context: "local".to_string(),
            selection: Selection::Full,
            files: Vec::new(),
            items: vec![command],
        };

        let results = execute(&plan);

        assert_eq!(results[0].status, TaskStatus::Failed);
        assert!(results[0].stderr.contains("command timed out after 1 second"));
    }

    #[test]
    fn zero_timeout_disables_the_deadline() {
        let env = BTreeMap::from([(COMMAND_TIMEOUT_ENV.to_string(), "0".to_string())]);
        assert_eq!(command_timeout(&env).expect("timeout"), None);
    }
}
