use crate::cache::{self, CacheConfig, CacheInputs};
use crate::checks;
use crate::checks::CheckOutcome;
use crate::model::{RunMode, TaskKind};
use crate::planner::PlanItem;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ExecutionOptions {
    pub cache_config: CacheConfig,
    pub mode: Option<RunMode>,
    pub profile: Option<String>,
    pub offline: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExecutionCacheStatus {
    Hit,
    Miss,
    Saved,
    Skipped { reason: String },
    Disabled,
}

impl std::fmt::Display for ExecutionCacheStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionCacheStatus::Hit => write!(f, "hit"),
            ExecutionCacheStatus::Miss => write!(f, "miss"),
            ExecutionCacheStatus::Saved => write!(f, "saved"),
            ExecutionCacheStatus::Skipped { reason } => write!(f, "skipped ({reason})"),
            ExecutionCacheStatus::Disabled => write!(f, "disabled"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub task_id: String,
    pub description: String,
    pub kind: String,
    pub phase: crate::model::Phase,
    pub outcome: CheckOutcome,
    pub stdout: String,
    pub stderr: String,
    pub cache: Option<ExecutionCacheStatus>,
}

/// Check if a plan item is eligible for caching
pub fn is_cacheable(item: &PlanItem) -> bool {
    use crate::model::TaskKind;
    // Verify phase only
    if item.phase != crate::model::Phase::Verify {
        return false;
    }
    // Must be a command task
    match &item.step.kind {
        TaskKind::Command { .. } => {}
        _ => return false,
    }
    true
}

pub fn execute_plan(
    items: &[PlanItem],
    root: &Path,
    options: &ExecutionOptions,
) -> Vec<ExecutionResult> {
    let cache_dir = if options.cache_config.enabled {
        Some(cache::cache_dir(&options.cache_config, root))
    } else {
        None
    };
    let mut results = Vec::new();
    let mut failed_chains: HashSet<String> = HashSet::new();
    let mut prerequisite_failures: HashMap<String, String> = HashMap::new();

    for item in items {
        if failed_chains.contains(&item.chain_id) && !item.step.always {
            results.push(ExecutionResult {
                task_id: item.task_id.clone(),
                description: item.description.clone(),
                kind: item.step.kind.description().to_string(),
                phase: item.phase,
                outcome: CheckOutcome::Skipped {
                    reason: prerequisite_failures
                        .get(&item.chain_id)
                        .cloned()
                        .unwrap_or_else(|| "skipped due to earlier failure in chain".to_string()),
                },
                stdout: String::new(),
                stderr: String::new(),
                cache: None,
            });
            continue;
        }

        let workdir = effective_workdir(item, root);
        let env = item.context.env.as_ref();
        let shell = item.context.shell.as_ref();

        // Determine if we should use cache for this item
        let use_cache = options.cache_config.enabled && is_cacheable(item);

        // Cache lookup: check if we have a cached result
        if use_cache {
            if let Some(ref cdir) = cache_dir {
                let inputs = build_cache_inputs(item, root, options);
                let key = cache::compute_key(&inputs);
                if let Some(entry) = cache::load(cdir, &key) {
                    results.push(ExecutionResult {
                        cache: Some(ExecutionCacheStatus::Hit),
                        task_id: item.task_id.clone(),
                        description: item.description.clone(),
                        kind: item.step.kind.description().to_string(),
                        phase: item.phase,
                        outcome: if entry.exit_code == 0 {
                            CheckOutcome::Passed
                        } else {
                            CheckOutcome::Failed {
                                reason: "cached failure".to_string(),
                            }
                        },
                        stdout: entry.stdout_summary.unwrap_or_default(),
                        stderr: entry.stderr_summary.unwrap_or_default(),
                    });
                    continue;
                }
            }
        }

        let check_result = match &item.step.kind {
            TaskKind::Command { command, expect } => {
                checks::command::run_command(command, expect.as_ref(), &workdir, env, shell)
            }
            TaskKind::FilesExist { paths } => checks::files::run_exist(paths, &workdir),
            TaskKind::FilesAbsent { paths } => checks::files::run_absent(paths, &workdir),
            TaskKind::ForbidText { paths, patterns } => {
                checks::text::run_forbid(paths, patterns, &workdir)
            }
            TaskKind::RequireText { paths, patterns } => {
                checks::text::run_require(paths, patterns, &workdir)
            }
        };

        if check_result.outcome.is_failure() {
            failed_chains.insert(item.chain_id.clone());
            if let Some(required_by) = &item.prerequisite_for {
                failed_chains.insert(required_by.clone());
                prerequisite_failures.insert(
                    required_by.clone(),
                    format!("skipped because prerequisite '{}' failed", item.chain_id),
                );
            }
        }

        // Save to cache on successful verify if applicable
        if use_cache {
            if let Some(ref cdir) = cache_dir {
                if let CheckOutcome::Passed = check_result.outcome {
                    let inputs = build_cache_inputs(item, root, options);
                    let key = cache::compute_key(&inputs);
                    let entry = cache::CacheEntry {
                        key: key.clone(),
                        task_id: item.task_id.clone(),
                        command: match &item.step.kind {
                            TaskKind::Command { command, .. } => command.clone(),
                            _ => vec![],
                        },
                        profile: options.profile.clone(),
                        phase: item.phase.to_string(),
                        mode: options.mode.map(|m| m.to_string()).unwrap_or_default(),
                        config_hash: inputs.config_hash.clone(),
                        exit_code: 0,
                        stdout_summary: Some(check_result.stdout.clone()),
                        stderr_summary: Some(check_result.stderr.clone()),
                        duration_ms: 0,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        invalidation_reason: None,
                        schema_version: cache::SCHEMA_VERSION,
                        tend_version: env!("CARGO_PKG_VERSION").to_string(),
                    };
                    let _ = cache::save(cdir, &entry);
                }
            }
        }

        let cache_status = if use_cache {
            Some(ExecutionCacheStatus::Saved)
        } else {
            None
        };

        results.push(ExecutionResult {
            task_id: item.task_id.clone(),
            description: item.description.clone(),
            kind: item.step.kind.description().to_string(),
            phase: item.phase,
            outcome: check_result.outcome,
            stdout: check_result.stdout,
            stderr: check_result.stderr,
            cache: cache_status,
        });
    }

    results
}

fn effective_workdir(item: &PlanItem, fallback: &Path) -> std::path::PathBuf {
    match &item.context.workdir {
        Some(policy) => {
            let config_dir = item.config_path.parent().unwrap_or(fallback);
            policy.resolve(config_dir, fallback)
        }
        None => item.config_path.parent().unwrap_or(fallback).to_path_buf(),
    }
}

/// Compute file hashes for a list of file paths.
/// Missing files are recorded as "MISSING:<path>".
/// Uses blake3 stable hashing.
fn compute_file_hashes(files: &[String], workdir: &Path) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for f in files {
        let path = if Path::new(f).is_absolute() {
            PathBuf::from(f)
        } else {
            workdir.join(f)
        };
        if path.exists() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let hash = blake3::hash(&bytes).to_hex().to_string();
                    result.push((f.clone(), hash));
                }
                Err(_) => {
                    result.push((f.clone(), format!("MISSING:{}", path.display())));
                }
            }
        } else {
            result.push((f.clone(), format!("MISSING:{}", path.display())));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

/// Compute a hash of the config file at the given path.
/// If `config_path` is a directory, looks for `.tend.json` inside it.
fn compute_config_hash(config_path: &Path) -> Option<String> {
    let path = if config_path.is_file() {
        config_path.to_path_buf()
    } else {
        config_path.join(".tend.json")
    };
    if path.exists() {
        std::fs::read(&path)
            .ok()
            .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
    } else {
        None
    }
}

/// Build `CacheInputs` from a plan item and execution options.
fn build_cache_inputs(item: &PlanItem, root: &Path, options: &ExecutionOptions) -> CacheInputs {
    let workdir = effective_workdir(item, root);
    let file_hashes = compute_file_hashes(&item.matched_files, &workdir);
    let config_hash = compute_config_hash(&item.config_path);
    let env_allowlist: Vec<(String, String)> = match &item.context.env {
        Some(env) => {
            let mut list: Vec<_> = env.clone().into_iter().collect();
            list.sort_by(|a, b| a.0.cmp(&b.0));
            list
        }
        None => vec![],
    };
    CacheInputs {
        schema_version: cache::SCHEMA_VERSION,
        tend_version: env!("CARGO_PKG_VERSION").to_string(),
        task_id: item.task_id.clone(),
        command: match &item.step.kind {
            TaskKind::Command { command, .. } => command.clone(),
            _ => vec![],
        },
        workdir,
        mode: options.mode.map(|m| m.to_string()).unwrap_or_default(),
        phase: item.phase.to_string(),
        profile: options.profile.clone(),
        config_hash,
        file_hashes,
        env_allowlist,
    }
}
