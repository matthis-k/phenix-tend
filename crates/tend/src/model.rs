use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Verify => "verify",
            Self::Fix => "fix",
            Self::Generate => "generate",
            Self::Setup => "setup",
            Self::Cleanup => "cleanup",
        })
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
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Changed => "changed",
            Self::Staged => "staged",
            Self::Full => "full",
            Self::GitRange => "git-range",
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileArgs {
    #[default]
    None,
    Matched,
    Selected,
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
    pub file_args: FileArgs,
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

const fn success_status() -> i32 {
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
    ContextNotAllowed {
        profile: String,
        context: String,
    },
    MissingImplementation {
        task: String,
        implementation: String,
    },
    PolicyViolation {
        task: String,
        context: String,
        reason: String,
    },
    MissingRevision(&'static str),
    Git(String),
    DependencyCycle(String),
    UnknownTask(String),
}

impl std::fmt::Display for TendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => formatter.write_str(message),
            Self::InvalidConfig(messages) => write!(
                formatter,
                "invalid Tend configuration:\n  - {}",
                messages.join("\n  - ")
            ),
            Self::UnknownProfile(profile) => write!(formatter, "unknown profile '{profile}'"),
            Self::UnknownContext(context) => {
                write!(formatter, "unknown execution context '{context}'")
            }
            Self::ContextNotAllowed { profile, context } => write!(
                formatter,
                "profile '{profile}' does not support execution context '{context}'"
            ),
            Self::MissingImplementation {
                task,
                implementation,
            } => write!(
                formatter,
                "task '{task}' has no '{implementation}' or 'default' implementation"
            ),
            Self::PolicyViolation {
                task,
                context,
                reason,
            } => write!(
                formatter,
                "task '{task}' is incompatible with execution context '{context}': {reason}"
            ),
            Self::MissingRevision(name) => write!(formatter, "selection requires --{name}"),
            Self::Git(message) => write!(formatter, "git error: {message}"),
            Self::DependencyCycle(task) => {
                write!(formatter, "dependency cycle involving task '{task}'")
            }
            Self::UnknownTask(task) => write!(formatter, "unknown task '{task}'"),
        }
    }
}

impl std::error::Error for TendError {}
