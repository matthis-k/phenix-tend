use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

use crate::graph;
use crate::model::{
    ExecutionContextConfig, FileArgs, Phase, TaskConfig, TaskImplementation, TaskKind, TendConfig,
    TendError, Workspace, CONFIG_VERSION,
};
use crate::selection;

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
            "unsupported config version {}; expected {CONFIG_VERSION}",
            config.version
        ));
    }
    if config.node.id.trim().is_empty() {
        errors.push("node id must not be empty".to_string());
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

    let mut seen_task_ids = HashSet::new();
    for task in &config.node.tasks {
        validate_task(task, &mut seen_task_ids, &mut errors);
    }

    let tasks: HashMap<&str, &TaskConfig> = config
        .node
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();

    for task in &config.node.tasks {
        for dependency in &task.requires {
            if !tasks.contains_key(dependency.as_str()) {
                errors.push(format!(
                    "task '{}' requires unknown task '{}'",
                    task.id, dependency
                ));
            }
        }
    }

    let all_task_ids: Vec<String> = config
        .node
        .tasks
        .iter()
        .map(|task| task.id.clone())
        .collect();
    if let Err(error) = graph::order_tasks(&all_task_ids, &tasks) {
        errors.push(error.to_string());
    }

    for (context_name, context) in &config.contexts {
        if context_name.trim().is_empty() {
            errors.push("execution context name must not be empty".to_string());
        }
        if context.implementation.trim().is_empty() {
            errors.push(format!(
                "execution context '{context_name}' must select an implementation"
            ));
        }
        validate_workdir(
            context.workdir.as_deref(),
            &format!("execution context '{context_name}'"),
            &mut errors,
        );
        validate_env(
            &context.env,
            &format!("execution context '{context_name}'"),
            &mut errors,
        );
    }

    for (profile_name, profile) in &config.profiles {
        if profile_name.trim().is_empty() {
            errors.push("profile name must not be empty".to_string());
        }
        if profile.tasks.is_empty() {
            errors.push(format!("profile '{profile_name}' selects no tasks"));
        }
        if profile.contexts.is_empty() {
            errors.push(format!(
                "profile '{profile_name}' allows no execution contexts"
            ));
        }
        validate_unique_values(
            &profile.tasks,
            &format!("profile '{profile_name}' task list"),
            &mut errors,
        );
        validate_unique_values(
            &profile.contexts,
            &format!("profile '{profile_name}' context list"),
            &mut errors,
        );

        for task_id in &profile.tasks {
            let Some(task) = tasks.get(task_id.as_str()).copied() else {
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

        let reachable_tasks = match graph::order_tasks(&profile.tasks, &tasks) {
            Ok(task_ids) => task_ids,
            Err(error) => {
                errors.push(format!("profile '{profile_name}': {error}"));
                Vec::new()
            }
        };

        for context_name in &profile.contexts {
            let Some(context) = config.contexts.get(context_name) else {
                errors.push(format!(
                    "profile '{profile_name}' references unknown context '{context_name}'"
                ));
                continue;
            };
            for task_id in &reachable_tasks {
                let Some(task) = tasks.get(task_id.as_str()).copied() else {
                    continue;
                };
                match resolve_implementation(task, context) {
                    Ok((_, implementation)) => {
                        if let Err(error) =
                            enforce_policy(task, implementation, context_name, context)
                        {
                            errors.push(error.to_string());
                        }
                    }
                    Err(error) => errors.push(error.to_string()),
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

fn validate_task(task: &TaskConfig, seen: &mut HashSet<String>, errors: &mut Vec<String>) {
    if task.id.trim().is_empty() {
        errors.push("task id must not be empty".to_string());
    } else if !seen.insert(task.id.clone()) {
        errors.push(format!("duplicate task id '{}'", task.id));
    }
    if task.implementations.is_empty() {
        errors.push(format!("task '{}' has no implementations", task.id));
    }
    if let Some(when) = &task.when {
        if let Err(error) = selection::validate_patterns(&when.changed.paths) {
            errors.push(format!("task '{}': {error}", task.id));
        }
    }

    for (implementation_name, implementation) in &task.implementations {
        let location = format!(
            "task '{}' implementation '{}'",
            task.id, implementation_name
        );
        if implementation_name.trim().is_empty() {
            errors.push(format!(
                "task '{}' has an empty implementation name",
                task.id
            ));
        }
        if task.phase == Phase::Verify && implementation.mutates {
            errors.push(format!("{location} mutates during the verify phase"));
        }
        if implementation.file_args != FileArgs::None
            && !matches!(implementation.kind, TaskKind::Command { .. })
        {
            errors.push(format!(
                "{location} requests file arguments for a non-command task"
            ));
        }
        validate_workdir(implementation.workdir.as_deref(), &location, errors);
        validate_env(&implementation.env, &location, errors);
        validate_kind(&implementation.kind, &location, errors);
    }
}

fn validate_kind(kind: &TaskKind, location: &str, errors: &mut Vec<String>) {
    match kind {
        TaskKind::Command { command, .. } if command.is_empty() => {
            errors.push(format!("{location} has an empty command"));
        }
        TaskKind::FilesExist { paths } | TaskKind::FilesAbsent { paths } if paths.is_empty() => {
            errors.push(format!("{location} has no paths"));
        }
        TaskKind::ForbidText { paths, patterns } | TaskKind::RequireText { paths, patterns } => {
            if paths.is_empty() {
                errors.push(format!("{location} has no paths"));
            }
            if patterns.is_empty() {
                errors.push(format!("{location} has no text patterns"));
            }
            if let Err(error) = selection::validate_patterns(paths) {
                errors.push(format!("{location}: {error}"));
            }
        }
        _ => {}
    }
}

fn validate_workdir(workdir: Option<&Path>, location: &str, errors: &mut Vec<String>) {
    let Some(workdir) = workdir else {
        return;
    };
    if workdir.is_absolute()
        || workdir
            .components()
            .any(|component| component == Component::ParentDir)
    {
        errors.push(format!(
            "{location} workdir '{}' must remain inside the repository root",
            workdir.display()
        ));
    }
}

fn validate_env(
    env: &std::collections::BTreeMap<String, String>,
    location: &str,
    errors: &mut Vec<String>,
) {
    for key in env.keys() {
        if key.trim().is_empty() || key.contains('=') || key.contains('\0') {
            errors.push(format!("{location} has invalid environment key '{key}'"));
        }
    }
}

fn validate_unique_values(values: &[String], location: &str, errors: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            errors.push(format!("{location} contains duplicate '{value}'"));
        }
    }
}

pub(crate) fn resolve_implementation<'a>(
    task: &'a TaskConfig,
    context: &ExecutionContextConfig,
) -> Result<(String, &'a TaskImplementation), TendError> {
    if let Some(implementation) = task.implementations.get(&context.implementation) {
        return Ok((context.implementation.clone(), implementation));
    }
    if let Some(implementation) = task.implementations.get("default") {
        return Ok(("default".to_string(), implementation));
    }
    Err(TendError::MissingImplementation {
        task: task.id.clone(),
        implementation: context.implementation.clone(),
    })
}

pub(crate) fn enforce_policy(
    task: &TaskConfig,
    implementation: &TaskImplementation,
    context_name: &str,
    context: &ExecutionContextConfig,
) -> Result<(), TendError> {
    let reason = if implementation.mutates && !context.allow_mutation {
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

    match reason {
        Some(reason) => Err(TendError::PolicyViolation {
            task: task.id.clone(),
            context: context_name.to_string(),
            reason: reason.to_string(),
        }),
        None => Ok(()),
    }
}
