use std::fs;
use std::path::Path;
use std::process::Command;

use tend::cache::CacheConfig;
use tend::discover;
use tend::execute;
use tend::execute::ExecutionCacheStatus;
use tend::execute::ExecutionOptions;
use tend::model::{Phase, PlanRequest, RunMode};
use tend::planner;

fn init_git_repo(path: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(path)
        .output()
        .expect("git init should succeed");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("git config should succeed");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .expect("git config should succeed");
}

fn make_plan_req() -> PlanRequest {
    PlanRequest {
        phase: Phase::Verify,
        mode: RunMode::Force,
        profile: None,
        group: None,
        target: None,
        files: vec![],
        offline: false,
        locked: false,
    }
}

#[test]
fn cache_hit_skips_command_execution() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    init_git_repo(root);

    let config = r#"{
        "version": 1,
        "node": {
            "id": "test",
            "tasks": [
                {
                    "id": "echo-task",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["echo", "hello"]
                }
            ]
        }
    }"#;
    fs::write(root.join(".tend.json"), config).unwrap();

    let cache_dir = root.join("cache");
    let cache_config = CacheConfig {
        enabled: true,
        dir: Some(cache_dir),
    };

    // First run: builds plan, executes, saves to cache
    let discovered = discover::discover_configs(root, None).unwrap();
    let nodes = discover::resolve_nodes(root, discovered);
    let req = make_plan_req();
    let plan = planner::build_plan(&nodes, &req).unwrap();

    let exec_opts = ExecutionOptions {
        cache_config: cache_config.clone(),
        mode: Some(RunMode::Force),
        profile: None,
        offline: false,
        locked: false,
    };
    let results1 = execute::execute_plan(&plan.items, root, &exec_opts);
    let r1 = results1.iter().find(|r| r.task_id == "echo-task").unwrap();
    assert!(r1.outcome.is_pass());
    assert_eq!(
        r1.cache,
        Some(ExecutionCacheStatus::Saved),
        "first run should save to cache"
    );

    // Second run: same plan items, same cache dir -- should hit cache
    let plan2 = planner::build_plan(&nodes, &req).unwrap();
    let results2 = execute::execute_plan(&plan2.items, root, &exec_opts);
    let r2 = results2.iter().find(|r| r.task_id == "echo-task").unwrap();
    assert!(r2.outcome.is_pass());
    assert_eq!(
        r2.cache,
        Some(ExecutionCacheStatus::Hit),
        "second run should hit cache"
    );
}

#[test]
fn no_cache_forces_execution() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    init_git_repo(root);

    let config = r#"{
        "version": 1,
        "node": {
            "id": "test",
            "tasks": [
                {
                    "id": "echo-task",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["echo", "hello"]
                }
            ]
        }
    }"#;
    fs::write(root.join(".tend.json"), config).unwrap();

    let cache_dir = root.join("cache");
    let enabled_cache = CacheConfig {
        enabled: true,
        dir: Some(cache_dir.clone()),
    };

    // First run WITH cache to establish entry
    let discovered = discover::discover_configs(root, None).unwrap();
    let nodes = discover::resolve_nodes(root, discovered);
    let req = make_plan_req();
    let plan = planner::build_plan(&nodes, &req).unwrap();

    let exec_opts_cache = ExecutionOptions {
        cache_config: enabled_cache.clone(),
        mode: Some(RunMode::Force),
        profile: None,
        offline: false,
        locked: false,
    };
    let results1 = execute::execute_plan(&plan.items, root, &exec_opts_cache);
    let r1 = results1.iter().find(|r| r.task_id == "echo-task").unwrap();
    assert_eq!(
        r1.cache,
        Some(ExecutionCacheStatus::Saved),
        "first run with cache should save"
    );

    // Now run with cache DISABLED
    let disabled_cache = CacheConfig {
        enabled: false,
        dir: Some(cache_dir),
    };
    let exec_opts_no_cache = ExecutionOptions {
        cache_config: disabled_cache,
        mode: Some(RunMode::Force),
        profile: None,
        offline: false,
        locked: false,
    };
    let plan2 = planner::build_plan(&nodes, &req).unwrap();
    let results2 = execute::execute_plan(&plan2.items, root, &exec_opts_no_cache);
    let r2 = results2.iter().find(|r| r.task_id == "echo-task").unwrap();
    assert!(r2.outcome.is_pass());
    assert_eq!(
        r2.cache, None,
        "no-cache should not report any cache status"
    );
}

#[test]
fn failed_task_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    init_git_repo(root);

    let config = r#"{
        "version": 1,
        "node": {
            "id": "test",
            "tasks": [
                {
                    "id": "failing-task",
                    "phase": "verify",
                    "kind": "command",
                    "command": ["false"]
                }
            ]
        }
    }"#;
    fs::write(root.join(".tend.json"), config).unwrap();

    let cache_dir = root.join("cache");
    let cache_config = CacheConfig {
        enabled: true,
        dir: Some(cache_dir.clone()),
    };

    let discovered = discover::discover_configs(root, None).unwrap();
    let nodes = discover::resolve_nodes(root, discovered);
    let req = make_plan_req();
    let plan = planner::build_plan(&nodes, &req).unwrap();

    let exec_opts = ExecutionOptions {
        cache_config: cache_config.clone(),
        mode: Some(RunMode::Force),
        profile: None,
        offline: false,
        locked: false,
    };

    // Run the failing task
    let results = execute::execute_plan(&plan.items, root, &exec_opts);
    let r = results
        .iter()
        .find(|r| r.task_id == "failing-task")
        .unwrap();
    assert!(r.outcome.is_failure(), "task should fail");

    // Cache dir should be empty (failed tasks should not be cached)
    let entry_count = tend::cache::count(&cache_dir).unwrap_or(0);
    assert_eq!(
        entry_count, 0,
        "no cache entries should exist for failed task"
    );

    // Run again -- should fail again, not hit a cached failure
    let results2 = execute::execute_plan(&plan.items, root, &exec_opts);
    let r2 = results2
        .iter()
        .find(|r| r.task_id == "failing-task")
        .unwrap();
    assert!(r2.outcome.is_failure(), "second run should also fail");
}
