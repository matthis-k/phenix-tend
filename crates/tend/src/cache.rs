use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Configuration for the task cache
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub enabled: bool,
    pub dir: Option<PathBuf>,
    pub default_mode: CacheDefaultMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheDefaultMode {
    Cache,
    NoCache,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None, // will use XDG cache dir or .tend/cache/
            default_mode: CacheDefaultMode::Cache,
        }
    }
}

/// Inputs used to compute the cache key
#[derive(Debug, Clone)]
pub struct CacheInputs {
    pub task_id: String,
    pub command: Vec<String>,
    pub workdir: PathBuf,
    pub mode: String,
    pub phase: String,
    pub profile: Option<String>,
    pub config_hash: Option<String>,
    pub file_hashes: Vec<(String, String)>, // (path, blake3_hash)
    pub env_allowlist: Vec<(String, String)>,
    pub tend_version: String,
    pub schema_version: u32,
}

/// A single cache entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub key: String,
    pub task_id: String,
    pub command: Vec<String>,
    pub profile: Option<String>,
    pub phase: String,
    pub mode: String,
    pub config_hash: Option<String>,
    pub exit_code: i32,
    pub stdout_summary: Option<String>,
    pub stderr_summary: Option<String>,
    pub duration_ms: u64,
    pub created_at: u64, // unix timestamp
    pub invalidation_reason: Option<String>,
}

/// Result of a cache lookup
#[derive(Debug)]
pub enum CacheResult {
    Hit(CacheEntry),
    Miss,
    Skipped(String), // reason
}

/// Compute a cache key from inputs
pub fn compute_key(inputs: &CacheInputs) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    inputs.task_id.hash(&mut hasher);
    inputs.command.hash(&mut hasher);
    inputs.workdir.to_string_lossy().hash(&mut hasher);
    inputs.mode.hash(&mut hasher);
    inputs.phase.hash(&mut hasher);
    inputs.profile.hash(&mut hasher);
    inputs.config_hash.hash(&mut hasher);
    inputs.schema_version.hash(&mut hasher);
    inputs.tend_version.hash(&mut hasher);
    for (path, hash) in &inputs.file_hashes {
        path.hash(&mut hasher);
        hash.hash(&mut hasher);
    }
    for (k, v) in &inputs.env_allowlist {
        k.hash(&mut hasher);
        v.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// Resolve the cache directory
pub fn cache_dir(config: &CacheConfig, workdir: &Path) -> PathBuf {
    if let Some(dir) = &config.dir {
        return dir.clone();
    }
    // Prefer repo-local .tend/cache/
    let local = workdir.join(".tend").join("cache");
    if local.exists() || workdir.join(".tend").exists() {
        return local;
    }
    // Fall back to XDG cache
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("phenix-tend").join("cache")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".cache")
            .join("phenix-tend")
            .join("cache")
    } else {
        workdir.join(".tend").join("cache")
    }
}

/// Load a cache entry by key
pub fn load(cache_dir: &Path, key: &str) -> Option<CacheEntry> {
    let path = cache_dir.join(format!("{}.json", key));
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Save a cache entry
pub fn save(cache_dir: &Path, entry: &CacheEntry) -> Result<(), String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("create cache dir: {e}"))?;
    let path = cache_dir.join(format!("{}.json", entry.key));
    let content =
        serde_json::to_string_pretty(entry).map_err(|e| format!("serialize cache: {e}"))?;
    std::fs::write(&path, &content).map_err(|e| format!("write cache: {e}"))?;
    Ok(())
}

/// Prune stale entries (older than max_age_secs)
pub fn prune(cache_dir: &Path, max_age_secs: u64) -> Result<usize, String> {
    if !cache_dir.exists() {
        return Ok(0);
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut pruned = 0usize;
    for entry in std::fs::read_dir(cache_dir).map_err(|e| format!("read cache dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(ce) = serde_json::from_str::<CacheEntry>(&content) {
                if now > ce.created_at && now - ce.created_at > max_age_secs {
                    let _ = std::fs::remove_file(&path);
                    pruned += 1;
                }
            }
        }
    }
    Ok(pruned)
}

/// Clear all cache entries
pub fn clear(cache_dir: &Path) -> Result<usize, String> {
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut cleared = 0usize;
    for entry in std::fs::read_dir(cache_dir).map_err(|e| format!("read cache dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let _ = std::fs::remove_file(&path);
            cleared += 1;
        }
    }
    Ok(cleared)
}

/// Get count of cache entries
pub fn count(cache_dir: &Path) -> Result<usize, String> {
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(cache_dir).map_err(|e| format!("read cache dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("json") {
            count += 1;
        }
    }
    Ok(count)
}

/// Check if a task should be cached based on its config
pub fn should_cache(
    mutates: Option<bool>,
    sandbox_safe: Option<bool>,
    interactive: Option<bool>,
) -> bool {
    // Never cache mutating tasks
    if mutates == Some(true) {
        return false;
    }
    // Never cache tasks that aren't sandbox-safe
    if sandbox_safe == Some(false) {
        return false;
    }
    // Never cache interactive tasks
    if interactive == Some(true) {
        return false;
    }
    // Default: cache safe read-only tasks
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_key_is_deterministic() {
        let inputs = CacheInputs {
            task_id: "test".into(),
            command: vec!["echo".into(), "hello".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: Some("git-hook".into()),
            config_hash: Some("abc123".into()),
            file_hashes: vec![("foo.rs".into(), "hash1".into())],
            env_allowlist: vec![],
            tend_version: "0.1.0".into(),
            schema_version: 1,
        };
        let key1 = compute_key(&inputs);
        let key2 = compute_key(&inputs);
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_different_inputs_different_keys() {
        let inputs1 = CacheInputs {
            task_id: "test".into(),
            command: vec!["echo".into(), "hello".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: None,
            config_hash: None,
            file_hashes: vec![],
            env_allowlist: vec![],
            tend_version: "0.1.0".into(),
            schema_version: 1,
        };
        let mut inputs2 = inputs1.clone();
        inputs2.task_id = "test2".into();
        assert_ne!(compute_key(&inputs1), compute_key(&inputs2));
    }

    #[test]
    fn test_mutating_task_not_cached() {
        assert!(!should_cache(Some(true), Some(true), Some(false)));
    }

    #[test]
    fn test_not_sandbox_safe_not_cached() {
        assert!(!should_cache(Some(false), Some(false), Some(false)));
    }

    #[test]
    fn test_read_only_sandbox_safe_is_cached() {
        assert!(should_cache(Some(false), Some(true), Some(false)));
    }

    #[test]
    fn test_default_is_cached() {
        assert!(should_cache(None, None, None));
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let entry = CacheEntry {
            key: "testkey".into(),
            task_id: "test".into(),
            command: vec!["echo".into()],
            profile: None,
            phase: "verify".into(),
            mode: "full".into(),
            config_hash: None,
            exit_code: 0,
            stdout_summary: Some("hello".into()),
            stderr_summary: None,
            duration_ms: 10,
            created_at: 1000,
            invalidation_reason: None,
        };
        save(dir.path(), &entry).expect("save");
        let loaded = load(dir.path(), "testkey").expect("load");
        assert_eq!(loaded.exit_code, 0);
        assert_eq!(loaded.stdout_summary, Some("hello".into()));
    }

    #[test]
    fn test_prune_removes_old_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let entry = CacheEntry {
            key: "old".into(),
            task_id: "old_test".into(),
            command: vec!["echo".into()],
            profile: None,
            phase: "verify".into(),
            mode: "full".into(),
            config_hash: None,
            exit_code: 0,
            stdout_summary: None,
            stderr_summary: None,
            duration_ms: 0,
            created_at: 100, // very old
            invalidation_reason: None,
        };
        save(dir.path(), &entry).expect("save");
        let pruned = prune(dir.path(), 10).expect("prune");
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_clear_removes_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..3 {
            let entry = CacheEntry {
                key: format!("key{}", i),
                task_id: "test".into(),
                command: vec!["echo".into()],
                profile: None,
                phase: "verify".into(),
                mode: "full".into(),
                config_hash: None,
                exit_code: 0,
                stdout_summary: None,
                stderr_summary: None,
                duration_ms: 0,
                created_at: 1000,
                invalidation_reason: None,
            };
            save(dir.path(), &entry).expect("save");
        }
        assert_eq!(clear(dir.path()).expect("clear"), 3);
    }
}
