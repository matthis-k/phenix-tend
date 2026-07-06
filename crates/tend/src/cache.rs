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
    pub schema_version: u32,
    pub tend_version: String,
}

/// Result of a cache lookup
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum CacheResult {
    Hit(Box<CacheEntry>),
    Miss,
    Skipped(String), // reason
}

pub const SCHEMA_VERSION: u32 = 1;

/// Compute a cache key from inputs using blake3
pub fn compute_key(inputs: &CacheInputs) -> String {
    let mut hasher = blake3::Hasher::new();

    // Write all inputs to the hasher
    hasher.update(inputs.schema_version.to_le_bytes().as_slice());
    hasher.update(inputs.tend_version.as_bytes());
    hasher.update(inputs.task_id.as_bytes());
    for cmd in &inputs.command {
        hasher.update(cmd.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(inputs.workdir.to_string_lossy().as_bytes());
    hasher.update(inputs.mode.as_bytes());
    hasher.update(inputs.phase.as_bytes());
    if let Some(ref profile) = inputs.profile {
        hasher.update(profile.as_bytes());
    }
    if let Some(ref ch) = inputs.config_hash {
        hasher.update(ch.as_bytes());
    }
    // Sort and hash file hashes for determinism
    let mut sorted_files: Vec<_> = inputs.file_hashes.clone();
    sorted_files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, hash) in &sorted_files {
        hasher.update(path.as_bytes());
        hasher.update(b":");
        hasher.update(hash.as_bytes());
        hasher.update(b"\0");
    }
    // Sort env allowlist keys
    let mut sorted_env: Vec<_> = inputs.env_allowlist.clone();
    sorted_env.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in &sorted_env {
        hasher.update(k.as_bytes());
        hasher.update(b"=");
        hasher.update(v.as_bytes());
        hasher.update(b"\0");
    }

    hasher.finalize().to_hex().to_string()
}

/// Get the cache key prefix used for stored keys
pub fn cache_key_prefix() -> String {
    format!("tend-cache-v{}-", SCHEMA_VERSION)
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

/// List entries matching a given task_id
pub fn list_entries_for_task(cache_dir: &Path, task_id: &str) -> Result<usize, String> {
    if !cache_dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(cache_dir).map_err(|e| format!("read cache dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(ce) = serde_json::from_str::<CacheEntry>(&content) {
                if ce.task_id == task_id {
                    count += 1;
                }
            }
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

    fn test_entry(key: &str) -> CacheEntry {
        CacheEntry {
            key: key.to_string(),
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
            schema_version: SCHEMA_VERSION,
            tend_version: "0.1.0".into(),
        }
    }

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
        let entry = test_entry("testkey");
        save(dir.path(), &entry).expect("save");
        let loaded = load(dir.path(), "testkey").expect("load");
        assert_eq!(loaded.exit_code, 0);
        assert_eq!(loaded.stdout_summary, None);
    }

    #[test]
    fn test_prune_removes_old_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut entry = test_entry("old");
        entry.created_at = 100; // very old
        save(dir.path(), &entry).expect("save");
        let pruned = prune(dir.path(), 10).expect("prune");
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_clear_removes_all() {
        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..3 {
            save(dir.path(), &test_entry(&format!("key{}", i))).expect("save");
        }
        assert_eq!(clear(dir.path()).expect("clear"), 3);
    }

    #[test]
    fn test_blake3_key_stable_across_runs() {
        let inputs = CacheInputs {
            schema_version: SCHEMA_VERSION,
            tend_version: "0.1.0".to_string(),
            task_id: "test".into(),
            command: vec!["echo".into(), "hello".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: Some("git-hook".into()),
            config_hash: Some("abc123".into()),
            file_hashes: vec![("foo.rs".into(), "hash1".into())],
            env_allowlist: vec![],
        };
        let key1 = compute_key(&inputs);
        let key2 = compute_key(&inputs);
        assert_eq!(key1, key2);
        // Should be a 64-char hex string (blake3 256-bit = 32 bytes = 64 hex chars)
        assert_eq!(key1.len(), 64);
    }

    #[test]
    fn test_changing_command_changes_key() {
        let base = CacheInputs {
            schema_version: SCHEMA_VERSION,
            tend_version: "0.1.0".to_string(),
            task_id: "test".into(),
            command: vec!["echo".into(), "hello".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: None,
            config_hash: None,
            file_hashes: vec![],
            env_allowlist: vec![],
        };
        let mut changed = base.clone();
        changed.command = vec!["echo".into(), "world".into()];
        assert_ne!(compute_key(&base), compute_key(&changed));
    }

    #[test]
    fn test_changing_file_hash_changes_key() {
        let base = CacheInputs {
            schema_version: SCHEMA_VERSION,
            tend_version: "0.1.0".to_string(),
            task_id: "test".into(),
            command: vec!["echo".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: None,
            config_hash: None,
            file_hashes: vec![],
            env_allowlist: vec![],
        };
        let mut changed = base.clone();
        changed.file_hashes = vec![("main.rs".into(), "hash_new".into())];
        assert_ne!(compute_key(&base), compute_key(&changed));
    }

    #[test]
    fn test_key_differs_without_file() {
        let base = CacheInputs {
            schema_version: SCHEMA_VERSION,
            tend_version: "0.1.0".to_string(),
            task_id: "test".into(),
            command: vec!["echo".into()],
            workdir: PathBuf::from("/tmp"),
            mode: "full".into(),
            phase: "verify".into(),
            profile: None,
            config_hash: None,
            file_hashes: vec![],
            env_allowlist: vec![],
        };
        let mut with_file = base.clone();
        with_file.file_hashes = vec![("missing.rs".into(), "MISSING:some/path".into())];
        assert_ne!(compute_key(&base), compute_key(&with_file));
    }

    #[test]
    fn test_mutating_task_not_cacheable() {
        assert!(!should_cache(Some(true), Some(true), Some(false)));
    }

    #[test]
    fn test_interactive_task_not_cacheable() {
        assert!(!should_cache(Some(false), Some(true), Some(true)));
    }

    #[test]
    fn test_not_sandbox_safe_not_cacheable() {
        assert!(!should_cache(Some(false), Some(false), Some(false)));
    }

    #[test]
    fn test_read_only_task_cacheable() {
        assert!(should_cache(Some(false), Some(true), Some(false)));
    }
}
