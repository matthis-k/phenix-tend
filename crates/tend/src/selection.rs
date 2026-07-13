use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::model::{Selection, TendError};

pub(crate) fn select_files(
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

pub(crate) fn match_files(
    patterns: &[String],
    files: &[String],
) -> Result<Vec<String>, TendError> {
    let globs = build_globs(patterns)?;
    Ok(files
        .iter()
        .filter(|file| globs.is_match(file.as_str()))
        .cloned()
        .collect())
}

pub(crate) fn validate_patterns(patterns: &[String]) -> Result<(), TendError> {
    build_globs(patterns).map(|_| ())
}

fn build_globs(patterns: &[String]) -> Result<GlobSet, TendError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|error| {
                TendError::InvalidConfig(vec![format!("invalid path glob '{pattern}': {error}")])
            })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| TendError::InvalidConfig(vec![error.to_string()]))
}

fn git_name_only(root: &Path, args: &[&str]) -> Result<Vec<String>, TendError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| TendError::Git(error.to_string()))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(TendError::Git(if message.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            message
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
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

    #[test]
    fn matching_preserves_file_order() {
        let files = vec![
            "src/lib.rs".to_string(),
            "README.md".to_string(),
            "src/main.rs".to_string(),
        ];
        let matched = match_files(&["src/**/*.rs".to_string()], &files).expect("match");
        assert_eq!(matched, vec!["src/lib.rs", "src/main.rs"]);
    }

    #[test]
    fn normalization_is_stable_and_unique() {
        let normalized = normalize_files(vec![
            "./src/lib.rs".to_string(),
            "src/lib.rs".to_string(),
            ".git/index".to_string(),
        ]);
        assert_eq!(normalized, vec!["src/lib.rs"]);
    }
}
