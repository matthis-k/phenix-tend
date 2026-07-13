use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::config::{enforce_policy, resolve_implementation};
use crate::graph;
use crate::model::{
    Plan, PlanItem, PlanReason, PlanRequest, Selection, TaskConfig, TendError, Workspace,
};
use crate::selection;

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
        Some(files) => normalize_explicit_files(files),
        None => selection::select_files(
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
    let mut reasons = BTreeMap::new();
    let mut matched_files = BTreeMap::new();
    for task_id in &profile.tasks {
        let task = tasks
            .get(task_id.as_str())
            .copied()
            .ok_or_else(|| TendError::UnknownTask(task_id.clone()))?;
        let Some((reason, matched)) = task_selection(task, profile.selection, &files)? else {
            continue;
        };
        roots.push(task.id.clone());
        reasons.insert(task.id.clone(), reason);
        matched_files.insert(task.id.clone(), matched);
    }

    let ordered = graph::order_tasks(&roots, &tasks)?;
    let mut items = Vec::with_capacity(ordered.len());
    for task_id in ordered {
        let task = tasks[task_id.as_str()];
        let (implementation_name, implementation) = resolve_implementation(task, context)?;
        enforce_policy(task, implementation, &request.context, context)?;

        let mut env = context.env.clone();
        env.extend(implementation.env.clone());
        env.insert("TEND_PROFILE".to_string(), request.profile.clone());
        env.insert("TEND_CONTEXT".to_string(), request.context.clone());
        env.insert("TEND_SELECTION".to_string(), profile.selection.to_string());
        env.insert(
            "TEND_FILES_JSON".to_string(),
            serde_json::to_string(&files).expect("serialize selected files"),
        );
        if let Some(base) = &request.base {
            env.insert("TEND_BASE".to_string(), base.clone());
        }
        if let Some(head) = &request.head {
            env.insert("TEND_HEAD".to_string(), head.clone());
        }

        let workdir = implementation
            .workdir
            .as_ref()
            .or(context.workdir.as_ref())
            .map(|path| workspace.root.join(path))
            .unwrap_or_else(|| workspace.root.clone());

        items.push(PlanItem {
            task_id: task.id.clone(),
            description: task.description.clone().unwrap_or_default(),
            phase: task.phase,
            implementation: implementation_name,
            kind: implementation.kind.clone(),
            workdir,
            env,
            requires: task.requires.clone(),
            reason: reasons
                .get(&task.id)
                .copied()
                .unwrap_or(PlanReason::Prerequisite),
            matched_files: matched_files.get(&task.id).cloned().unwrap_or_default(),
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

fn task_selection(
    task: &TaskConfig,
    selection: Selection,
    files: &[String],
) -> Result<Option<(PlanReason, Vec<String>)>, TendError> {
    if selection == Selection::Full {
        return Ok(Some((PlanReason::Full, Vec::new())));
    }
    if task.always {
        return Ok(Some((PlanReason::Always, Vec::new())));
    }
    let Some(when) = &task.when else {
        return Ok(Some((PlanReason::Unconditional, Vec::new())));
    };
    let matched = selection::match_files(&when.changed.paths, files)?;
    Ok((!matched.is_empty()).then_some((PlanReason::ChangedFile, matched)))
}

fn normalize_explicit_files(files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|file| file.trim_start_matches("./").to_string())
        .filter(|file| !file.is_empty() && !file.starts_with(".git/"))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ChangedConfig, ExecutionContextConfig, NodeConfig, Phase, ProfileConfig,
        TaskImplementation, TaskKind, TendConfig, WhenConfig,
    };
    use std::path::PathBuf;

    fn workspace() -> Workspace {
        let typecheck = TaskConfig {
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
                version: 2,
                profiles: BTreeMap::from([(
                    "verify".to_string(),
                    ProfileConfig {
                        phase: Phase::Verify,
                        selection: Selection::Changed,
                        tasks: vec!["typecheck".to_string()],
                        contexts: vec!["sandbox".to_string()],
                    },
                )]),
                contexts: BTreeMap::from([(
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
                )]),
                node: NodeConfig {
                    id: "root".to_string(),
                    description: None,
                    tasks: vec![typecheck],
                },
            },
        }
    }

    #[test]
    fn context_selects_the_task_implementation() {
        let plan = plan(
            &workspace(),
            &PlanRequest {
                profile: "verify".to_string(),
                context: "sandbox".to_string(),
                base: None,
                head: None,
                files: Some(vec!["src/lib.rs".to_string()]),
            },
        )
        .expect("plan");
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].implementation, "direct");
    }

    #[test]
    fn non_matching_changes_produce_an_empty_plan() {
        let plan = plan(
            &workspace(),
            &PlanRequest {
                profile: "verify".to_string(),
                context: "sandbox".to_string(),
                base: None,
                head: None,
                files: Some(vec!["README.md".to_string()]),
            },
        )
        .expect("plan");
        assert!(plan.items.is_empty());
    }
}
