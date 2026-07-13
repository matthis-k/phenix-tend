use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use globset::{GlobBuilder, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

pub const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Verify,
    Fix,
    Generate,
    Setup,
    Cleanup,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Verify => "verify",
            Self::Fix => "fix",
            Self::Generate => "generate",
            Self::Setup => "setup",
            Self::Cleanup => "cleanup",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Selection {
    Changed,
    Staged,
    Full,
    GitRange,
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Changed => "changed",
            Self::Staged => "staged",
            Self::Full => "full",
            Self::GitRange => "git-range",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TendConfig {
    pub version: u32,
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub contexts: BTreeMap<String, ExecutionContextConfig>,
    pub node: NodeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub phase: Phase,
    pub selection: Selection,
    pub tasks: Vec<String>,
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContextConfig {
    pub implementation: String,
    #[serde(default)]
    pub allow_mutation: bool,
    #[serde(default)]
    pub allow_interactive: bool,
    #[serde(default)]
    pub allow_network: bool,
    #[serde(default)]
    pub require_sandbox_safe: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub workdir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub description: Option<String>,
    pub when: Option<WhenConfig>,
    pub tasks: Vec<TaskConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhenConfig {
    pub changed: ChangedConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedConfig {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    pub id: String,
    pub description: Option<String>,
    pub phase: Phase,
    #[serde(default)]
    pub tags: Vec<String>,
    pub when: Option<WhenConfig>,
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub requires: Vec<String>,
    pub implementations: BTreeMap<String, TaskImplementation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskImplementation {
    #[serde(flatten)]
    pub kind: TaskKind,
    #[serde(default)]
    pub mutates: bool,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub sandbox_safe: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub workdir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskKind {
    Command {
        command: Vec<String>,
        #[serde(default = "success_status")]
        expect_status: i32,
    },
    FilesExist {
        paths: Vec<String>,
    },
    FilesAbsent {
        paths: Vec<String>,
    },
    ForbidText {
        paths: Vec<String>,
        patterns: Vec<String>,
    },
    RequireText {
        paths: Vec<String>,
        patterns: Vec<String>,
    },
}

fn success_status() -> i32 {
    0
}

impl TaskKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::FilesExist { .. } => "filesExist",
            Self::FilesAbsent { .. } => "filesAbsent",
            Self::ForbidText { .. } => "forbidText",
            Self::RequireText { .. } => "requireText",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub config_path: PathBuf,
    pub config: TendConfig,
}

#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub profile: String,
    pub context: String,
    pub base: Option<String>,
    pub head: Option<String>,
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub profile: String,
    pub context: String,
    pub selection: Selection,
    pub files: Vec<String>,
    pub items: Vec<PlanItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub task_id: String,
    pub description: String,
    pub phase: Phase,
    pub implementation: String,
    pub kind: TaskKind,
    pub workdir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub requires: Vec<String>,
    pub reason: PlanReason,
    pub matched_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanReason {
    Full,
    Always,
    ChangedFile,
    Prerequisite,
    Unconditional,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug)]
pub enum TendError {
    Io(String),
    InvalidConfig(Vec<String>),
    UnknownProfile(String),
    UnknownContext(String),
    ContextNotAllowed { profile: String, context: String },
    MissingImplementation { task: String, implementation: String },
    PolicyViolation { task: String, context: String, reason: String },
    MissingRevision(&'static str),
    Git(String),
    DependencyCycle(String),
    UnknownTask(String),
}

impl std::fmt::Display for TendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => f.write_str(message),
            Self::InvalidConfig(messages) => {
                write!(f, "invalid Tend configuration:\n  - {}", messages.join("\n  - "))
            }
            Self::UnknownProfile(profile) => write!(f, "unknown profile '{profile}'"),
            Self::UnknownContext(context) => write!(f, "unknown execution context '{context}'"),
            Self::ContextNotAllowed { profile, context } => write!(
                f,
                "profile '{profile}' does not support execution context '{context}'"
            ),
            Self::MissingImplementation {
                task,
                implementation,
            } => write!(
                f,
                "task '{task}' has no '{implementation}' or 'default' implementation"
            ),
            Self::PolicyViolation {
                task,
                context,
                reason,
            } => write!(
                f,
                "task '{task}' is incompatible with execution context '{context}': {reason}"
            ),
            Self::MissingRevision(name) => write!(f, "selection requires --{name}"),
            Self::Git(message) => write!(f, "git error: {message}"),
            Self::DependencyCycle(task) => {
                write!(f, "dependency cycle involving task '{task}'")
            }
            Self::UnknownTask(task) => write!(f, "unknown task '{task}'"),
        }
    }
}

impl std::error::Error for TendError {}

pub fn load(root: impl AsRef<Path>) -> Result<Workspace, TendError> {
    let root = root
        .as_ref()
        .canonicalize()
        .map_err(|error| TendError::Io(format!("resolve root: {error}")))?;
    let config_path = root.join(".tend.json");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|error| TendError::Io(format!("read {}: {error}", config_path.display())))?;
    let config: TendConfig = serde_json::from_str(&content)
        .map_err(|error| TendError::Io(format!("parse {}: {error}", config_path.display())))?;
    let workspace = Workspace {
        root,
        config_path,
        config,
    };
    validate(&workspace)?;
    Ok(workspace)
}

pub fn validate(workspace: &Workspace) -> Result<(), TendError> {
    let config = &workspace.config;
    let mut errors = Vec::new();

    if config.version != CONFIG_VERSION {
        errors.push(format!(
            "unsupported config version {}; expected {}",
            config.version, CONFIG_VERSION
        ));
    }
    if config.profiles.is_empty() {
        errors.push("at least one profile is required".to_string());
    }
    if config.contexts.is_empty() {
        errors.push("at least one execution context is required".to_string());
    }
    if config.node.tasks.is_empty() {
        errors.push("at least one task is required".to_string());
    }

    let mut task_ids = HashSet::new();
    let tasks: HashMap<&str, &TaskConfig> = config
        .node
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();

    for task in &config.node.tasks {
        if !task_ids.insert(task.id.as_str()) {
            errors.push(format!("duplicate task id '{}'", task.id));
        }
        if task.implementations.is_empty() {
            errors.push(format!("task '{}' has no implementations", task.id));
        }
        for (name, implementation) in &task.implementations {
            if name.trim().is_empty() {
                errors.push(format!("task '{}' has an empty implementation name", task.id));
            }
            if let TaskKind::Command { command, .. } = &implementation.kind {
                if command.is_empty() {
                    errors.push(format!(
                        "task '{}' implementation '{}' has an empty command",
                        task.id, name
                    ));
                }
            }
        }
        for dependency in &task.requires {
            if !tasks.contains_key(dependency.as_str()) {
                errors.push(format!(
                    "task '{}' requires unknown task '{}'",
                    task.id, dependency
                ));
            }
        }
    }

    for (profile_name, profile) in &config.profiles {
        if profile.tasks.is_empty() {
            errors.push(format!("profile '{profile_name}' selects no tasks"));
        }
        if profile.contexts.is_empty() {
            errors.push(format!("profile '{profile_name}' allows no execution contexts"));
        }
        for task_id in &profile.tasks {
            let Some(task) = tasks.get(task_id.as_str()) else {
                errors.push(format!(
                    "profile '{profile_name}' selects unknown task '{task_id}'"
                ));
                continue;
            };
            if task.phase != profile.phase {
                errors.push(format!(
                    "profile '{profile_name}' phase '{}' does not match task '{}' phase '{}'",
                    profile.phase, task.id, task.phase
                ));
            }
        }
        for context_name in &profile.contexts {
            let Some(context) = config.contexts.get(context_name) else {
                errors.push(format!(
                    "profile '{profile_name}' references unknown context '{context_name}'"
                ));
                continue;
            };
            for task_id in &profile.tasks {
                if let Some(task) = tasks.get(task_id.as_str()) {
                    match resolve_implementation(task, context) {
                        Ok((_, implementation)) => {
                            if let Err(error) = enforce_policy(
                                task,
                                implementation,
                                context_name,
                                context,
                            ) {
                                errors.push(error.to_string());
                            }
                        }
                        Err(error) => errors.push(error.to_string()),
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(TendError::InvalidConfig(errors))
    }
}

pub fn plan(workspace: &Workspace, request: &PlanRequest) -> Result<Plan, TendError> {
    let profile = workspace
        .config
        .profiles
        .get(&request.profile)
        .ok_or_else(|| TendError::UnknownProfile(request.profile.clone()))?;
    let context = workspace
        .config
        .contexts
        .get(&request.context)
        .ok_or_else(|| TendError::UnknownContext(request.context.clone()))?;

    if !profile.contexts.iter().any(|name| name == &request.context) {
        return Err(TendError::ContextNotAllowed {
            profile: request.profile.clone(),
            context: request.context.clone(),
        });
    }

    let files = match &request.files {
        Some(files) => normalize_files(files.clone()),
        None => select_files(
            &workspace.root,
            profile.selection,
            request.base.as_deref(),
            request.head.as_deref(),
        )?,
    };

    let tasks: HashMap<&str, &TaskConfig> = workspace
        .config
        .node
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();

    let mut roots = Vec::new();
    let mut reasons = HashMap::new();
    let mut matched = HashMap::new();

    for task_id in &profile.tasks {
        let task = tasks
            .get(task_id.as_str())
            .copied()
            .ok_or_else(|| TendError::UnknownTask(task_id.clone()))?;
        if task.phase != profile.phase {
            continue;
        }
        let (applies, reason, matched_files) = task_applies(task, profile.selection, &files)?;
        if applies {
            roots.push(task.id.clone());
            reasons.insert(task.id.clone(), reason);
            matched.insert(task.id.clone(), matched_files);
        }
    }

    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for root in roots {
        visit_task(
            &root,
            &tasks,
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }

    let mut items = Vec::new();
    for task_id in ordered {
        let task = tasks[task_id.as_str()];
        let (implementation_name, implementation) = resolve_implementation(task, context)?;
        enforce_policy(task, implementation, &request.context, context)?;

        let mut env = context.env.clone();
        env.extend(implementation.env.clone());
        let workdir = implementation
            .workdir
            .as_ref()
            .or(context.workdir.as_ref())
            .map(|path| workspace.root.join(path))
            .unwrap_or_else(|| workspace.root.clone());

        let reason = reasons
            .get(&task.id)
            .copied()
            .unwrap_or(PlanReason::Prerequisite);
        let matched_files = matched.get(&task.id).cloned().unwrap_or_default();

        items.push(PlanItem {
            task_id: task.id.clone(),
            description: task.description.clone().unwrap_or_default(),
            phase: task.phase,
            implementation: implementation_name.to_string(),
            kind: implementation.kind.clone(),
            workdir,
            env,
            requires: task.requires.clone(),
            reason,
            matched_files,
        });
    }

    Ok(Plan {
        profile: request.profile.clone(),
        context: request.context.clone(),
        selection: profile.selection,
        files,
        items,
    })
}

pub fn execute(plan: &Plan) -> Vec<TaskResult> {
    let mut results = Vec::new();
    let mut status_by_task = HashMap::new();

    for item in &plan.items {
        if item.requires.iter().any(|dependency| {
            status_by_task.get(dependency) != Some(&TaskStatus::Passed)
        }) {
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
    results.iter().any(|result| result.status == TaskStatus::Failed)
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
    if command.is_empty() {
        return Err(CapturedOutput {
            stdout: String::new(),
            stderr: "empty command".to_string(),
        });
    }

    let output = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(workdir)
        .envs(env)
        .output();

    match output {
        Ok(output) => captured_command_result(output, expect_status),
        Err(error) => Err(CapturedOutput {
            stdout: String::new(),
            stderr: format!("spawn '{}': {error}", command[0]),
        }),
    }
}

fn captured_command_result(
    output: Output,
    expect_status: i32,
) -> Result<CapturedOutput, CapturedOutput> {
    let captured = CapturedOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    if output.status.code() == Some(expect_status) {
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
        .filter(|path| workdir.join(path).exists() != should_exist)
        .cloned()
        .collect();
    if mismatches.is_empty() {
        Ok(CapturedOutput {
            stdout: String::new(),
            stderr: String::new(),
        })
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
    let mut failures = Vec::new();
    for pattern in patterns {
        let found = files.iter().any(|path| {
            std::fs::read_to_string(path)
                .map(|content| content.contains(pattern))
                .unwrap_or(false)
        });
        if found != require {
            failures.push(pattern.clone());
        }
    }
    if failures.is_empty() {
        Ok(CapturedOutput {
            stdout: String::new(),
            stderr: String::new(),
        })
    } else {
        Err(CapturedOutput {
            stdout: String::new(),
            stderr: format!("text expectation failed: {}", failures.join(", ")),
        })
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
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(root).ok()?;
            globs.is_match(relative).then(|| entry.path().to_path_buf())
        })
        .collect()
}

fn resolve_implementation<'a>(
    task: &'a TaskConfig,
    context: &ExecutionContextConfig,
) -> Result<(&'a str, &'a TaskImplementation), TendError> {
    if let Some(implementation) = task.implementations.get(&context.implementation) {
        return Ok((context.implementation.as_str(), implementation));
    }
    if let Some(implementation) = task.implementations.get("default") {
        return Ok(("default", implementation));
    }
    Err(TendError::MissingImplementation {
        task: task.id.clone(),
        implementation: context.implementation.clone(),
    })
}

fn enforce_policy(
    task: &TaskConfig,
    implementation: &TaskImplementation,
    context_name: &str,
    context: &ExecutionContextConfig,
) -> Result<(), TendError> {
    let violation = if implementation.mutates && !context.allow_mutation {
        Some("mutation is not allowed")
    } else if implementation.interactive && !context.allow_interactive {
        Some("interactive execution is not allowed")
    } else if implementation.network && !context.allow_network {
        Some("network access is not allowed")
    } else if context.require_sandbox_safe && !implementation.sandbox_safe {
        Some("implementation is not marked sandbox-safe")
    } else {
        None
    };

    if let Some(reason) = violation {
        Err(TendError::PolicyViolation {
            task: task.id.clone(),
            context: context_name.to_string(),
            reason: reason.to_string(),
        })
    } else {
        Ok(())
    }
}

fn visit_task(
    task_id: &str,
    tasks: &HashMap<&str, &TaskConfig>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) -> Result<(), TendError> {
    if visited.contains(task_id) {
        return Ok(());
    }
    if !visiting.insert(task_id.to_string()) {
        return Err(TendError::DependencyCycle(task_id.to_string()));
    }
    let task = tasks
        .get(task_id)
        .copied()
        .ok_or_else(|| TendError::UnknownTask(task_id.to_string()))?;
    for dependency in &task.requires {
        visit_task(dependency, tasks, visiting, visited, ordered)?;
    }
    visiting.remove(task_id);
    visited.insert(task_id.to_string());
    ordered.push(task_id.to_string());
    Ok(())
}

fn task_applies(
    task: &TaskConfig,
    selection: Selection,
    files: &[String],
) -> Result<(bool, PlanReason, Vec<String>), TendError> {
    if selection == Selection::Full {
        return Ok((true, PlanReason::Full, Vec::new()));
    }
    if task.always {
        return Ok((true, PlanReason::Always, Vec::new()));
    }
    let Some(when) = &task.when else {
        return Ok((true, PlanReason::Unconditional, Vec::new()));
    };
    let matched = match_files(&when.changed.paths, files)?;
    Ok((
        !matched.is_empty(),
        PlanReason::ChangedFile,
        matched,
    ))
}

fn match_files(patterns: &[String], files: &[String]) -> Result<Vec<String>, TendError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|error| TendError::InvalidConfig(vec![format!(
                "invalid path glob '{pattern}': {error}"
            )]))?;
        builder.add(glob);
    }
    let globs = builder
        .build()
        .map_err(|error| TendError::InvalidConfig(vec![error.to_string()]))?;
    Ok(files
        .iter()
        .filter(|file| globs.is_match(file))
        .cloned()
        .collect())
}

fn select_files(
    root: &Path,
    selection: Selection,
    base: Option<&str>,
    head: Option<&str>,
) -> Result<Vec<String>, TendError> {
    match selection {
        Selection::Full => Ok(Vec::new()),
        Selection::Changed => {
            let mut files = git_name_only(root, &["diff", "--name-only", "--diff-filter=ACMR"])?;
            files.extend(git_name_only(
                root,
                &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
            )?);
            files.extend(git_name_only(
                root,
                &["ls-files", "--others", "--exclude-standard"],
            )?);
            Ok(normalize_files(files))
        }
        Selection::Staged => git_name_only(
            root,
            &["diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        )
        .map(normalize_files),
        Selection::GitRange => {
            let base = base.ok_or(TendError::MissingRevision("base"))?;
            let head = head.ok_or(TendError::MissingRevision("head"))?;
            git_check_range(root, base, head)?;
            git_name_only(
                root,
                &[
                    "diff",
                    "--name-only",
                    "--diff-filter=ACMR",
                    base,
                    head,
                ],
            )
            .map(normalize_files)
        }
    }
}

fn git_name_only(root: &Path, args: &[&str]) -> Result<Vec<String>, TendError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| TendError::Git(error.to_string()))?;
    if !output.status.success() {
        return Err(TendError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn git_check_range(root: &Path, base: &str, head: &str) -> Result<(), TendError> {
    let output = Command::new("git")
        .args(["diff", "--check", base, head])
        .current_dir(root)
        .output()
        .map_err(|error| TendError::Git(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(TendError::Git(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }
}

fn normalize_files(files: Vec<String>) -> Vec<String> {
    let mut files: BTreeSet<String> = files
        .into_iter()
        .map(|file| file.trim_start_matches("./").to_string())
        .filter(|file| !file.is_empty())
        .collect();
    files.retain(|file| !file.starts_with(".git/"));
    files.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Workspace {
        let task = TaskConfig {
            id: "typecheck".to_string(),
            description: None,
            phase: Phase::Verify,
            tags: Vec::new(),
            when: Some(WhenConfig {
                changed: ChangedConfig {
                    paths: vec!["src/**/*.rs".to_string()],
                },
            }),
            always: false,
            requires: Vec::new(),
            implementations: BTreeMap::from([
                (
                    "nix".to_string(),
                    TaskImplementation {
                        kind: TaskKind::Command {
                            command: vec!["nix".to_string(), "build".to_string()],
                            expect_status: 0,
                        },
                        mutates: false,
                        interactive: false,
                        network: true,
                        sandbox_safe: false,
                        env: BTreeMap::new(),
                        workdir: None,
                    },
                ),
                (
                    "direct".to_string(),
                    TaskImplementation {
                        kind: TaskKind::Command {
                            command: vec!["cargo".to_string(), "check".to_string()],
                            expect_status: 0,
                        },
                        mutates: false,
                        interactive: false,
                        network: false,
                        sandbox_safe: true,
                        env: BTreeMap::new(),
                        workdir: None,
                    },
                ),
            ]),
        };
        Workspace {
            root: PathBuf::from("/tmp"),
            config_path: PathBuf::from("/tmp/.tend.json"),
            config: TendConfig {
                version: CONFIG_VERSION,
                profiles: BTreeMap::from([(
                    "full".to_string(),
                    ProfileConfig {
                        phase: Phase::Verify,
                        selection: Selection::Full,
                        tasks: vec!["typecheck".to_string()],
                        contexts: vec!["local".to_string(), "sandbox".to_string()],
                    },
                )]),
                contexts: BTreeMap::from([
                    (
                        "local".to_string(),
                        ExecutionContextConfig {
                            implementation: "nix".to_string(),
                            allow_mutation: false,
                            allow_interactive: false,
                            allow_network: true,
                            require_sandbox_safe: false,
                            env: BTreeMap::new(),
                            workdir: None,
                        },
                    ),
                    (
                        "sandbox".to_string(),
                        ExecutionContextConfig {
                            implementation: "direct".to_string(),
                            allow_mutation: false,
                            allow_interactive: false,
                            allow_network: false,
                            require_sandbox_safe: true,
                            env: BTreeMap::new(),
                            workdir: None,
                        },
                    ),
                ]),
                node: NodeConfig {
                    id: "root".to_string(),
                    description: None,
                    when: None,
                    tasks: vec![task],
                },
            },
        }
    }

    #[test]
    fn context_selects_implementation() {
        let workspace = workspace();
        let plan = plan(
            &workspace,
            &PlanRequest {
                profile: "full".to_string(),
                context: "sandbox".to_string(),
                base: None,
                head: None,
                files: None,
            },
        )
        .expect("plan");
        assert_eq!(plan.items[0].implementation, "direct");
        match &plan.items[0].kind {
            TaskKind::Command { command, .. } => assert_eq!(command[0], "cargo"),
            _ => panic!("expected command"),
        }
    }

    #[test]
    fn validation_rejects_policy_mismatch() {
        let mut workspace = workspace();
        workspace.config.contexts.get_mut("sandbox").unwrap().implementation = "nix".to_string();
        let error = validate(&workspace).expect_err("network implementation must be rejected");
        assert!(error.to_string().contains("network access is not allowed"));
    }

    #[test]
    fn default_implementation_is_fallback() {
        let mut workspace = workspace();
        let task = workspace.config.node.tasks.first_mut().unwrap();
        let direct = task.implementations.remove("direct").unwrap();
        task.implementations.insert("default".to_string(), direct);
        let plan = plan(
            &workspace,
            &PlanRequest {
                profile: "full".to_string(),
                context: "sandbox".to_string(),
                base: None,
                head: None,
                files: None,
            },
        )
        .expect("plan");
        assert_eq!(plan.items[0].implementation, "default");
    }

    #[test]
    fn changed_selection_matches_globs() {
        let task = &workspace().config.node.tasks[0];
        let (applies, _, matched) = task_applies(
            task,
            Selection::Changed,
            &["src/lib/core.rs".to_string(), "README.md".to_string()],
        )
        .expect("match");
        assert!(applies);
        assert_eq!(matched, vec!["src/lib/core.rs"]);
    }

    #[test]
    fn dependency_cycles_are_rejected() {
        let mut workspace = workspace();
        let task = workspace.config.node.tasks.first_mut().unwrap();
        task.requires = vec!["typecheck".to_string()];
        let error = plan(
            &workspace,
            &PlanRequest {
                profile: "full".to_string(),
                context: "local".to_string(),
                base: None,
                head: None,
                files: None,
            },
        )
        .expect_err("cycle");
        assert!(matches!(error, TendError::DependencyCycle(_)));
    }
}
