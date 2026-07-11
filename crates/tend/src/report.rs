use crate::checks::CheckOutcome;
use crate::execute::{ExecutionCacheStatus, ExecutionResult};

fn cache_tag(cache: &Option<ExecutionCacheStatus>) -> String {
    match cache {
        Some(ExecutionCacheStatus::Hit) => "[cached]".to_string(),
        Some(ExecutionCacheStatus::Saved) => "[saved]".to_string(),
        Some(ExecutionCacheStatus::Skipped { reason }) => format!("[cache: {}]", reason),
        Some(ExecutionCacheStatus::Disabled) => String::new(),
        Some(ExecutionCacheStatus::Miss) => String::new(),
        None => String::new(),
    }
}

pub fn print_results(results: &[ExecutionResult], verbose: bool) -> (usize, usize, usize) {
    let mut failed = 0usize;
    let mut passed = 0usize;
    let mut skipped = 0usize;

    for r in results {
        match &r.outcome {
            CheckOutcome::Skipped { reason } => {
                skipped += 1;
                let cache_tag = cache_tag(&r.cache);
                if !cache_tag.is_empty() {
                    println!("SKIPPED {} {}", r.task_id, cache_tag);
                } else if verbose {
                    println!("SKIPPED {} (reason: {})", r.task_id, reason);
                }
            }
            CheckOutcome::Passed => {
                passed += 1;
                let cache_tag = cache_tag(&r.cache);
                if !cache_tag.is_empty() {
                    println!("PASSED {} {}", r.task_id, cache_tag);
                }
            }
            CheckOutcome::Failed { reason } | CheckOutcome::Errored { reason } => {
                failed += 1;
                let cache_tag = cache_tag(&r.cache);
                println!("FAILED {} {}", r.task_id, cache_tag);
                if !r.description.is_empty() {
                    println!("  description: {}", r.description);
                }
                println!("  phase: {}", r.phase);
                println!("  kind: {}", r.kind);
                if !reason.is_empty() {
                    println!("  reason: {}", reason);
                }
                if !r.stdout.is_empty() && verbose {
                    for line in r.stdout.lines() {
                        println!("  stdout: {}", line);
                    }
                }
                if !r.stderr.is_empty() {
                    for line in r.stderr.lines() {
                        println!("  stderr: {}", line);
                    }
                }
                println!();
            }
        }
    }

    if verbose {
        for r in results {
            if r.outcome.is_pass() {
                println!("PASSED {}", r.task_id);
            }
        }
    }

    let cached = results
        .iter()
        .filter(|r| matches!(&r.cache, Some(ExecutionCacheStatus::Hit)))
        .count();

    println!("Summary:");
    println!("  failed: {}", failed);
    println!("  passed: {}", passed);
    println!("  skipped: {}", skipped);
    if cached > 0 {
        println!("  cached: {}", cached);
    }

    (failed, passed, skipped)
}
