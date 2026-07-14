# Tend

Tend is a deterministic task planner and runner for repository quality gates.

Schema v2 separates logical tasks, profiles, execution contexts, and concrete implementations. See [ARCHITECTURE.md](ARCHITECTURE.md) for the model and [MIGRATION.md](MIGRATION.md) for a complete migration example.

## Commands

```sh
# Validate all profiles, contexts, tasks, dependencies, and policies
tend validate

# Inspect a resolved plan
tend plan --profile manual --context local

# Execute a profile
tend check --profile manual --context local

# Execute an exact CI revision range
tend check \
  --profile ci \
  --context local \
  --base "$BASE_SHA" \
  --head "$HEAD_SHA"

# Inspect the configured model
tend list
```

Every `check` names both dimensions explicitly:

- `--profile` selects what should run.
- `--context` selects how it may run.

## Process contract

Tend uses stable process exit classes so wrappers and CI jobs do not need to parse output:

- `0`: configuration and selected tasks succeeded.
- `1`: at least one selected task failed.
- `2`: Tend could not load, validate, plan, or execute the request because of a configuration, selection, Git, or I/O error.

Command tasks receive planner metadata through environment variables:

- `TEND_PROFILE`: selected profile name.
- `TEND_CONTEXT`: selected execution-context name.
- `TEND_SELECTION`: profile selection strategy (`changed`, `staged`, `full`, or `git-range`).
- `TEND_BASE`: base revision when `--base` was supplied.
- `TEND_HEAD`: head revision when `--head` was supplied.

These variables are metadata for task implementations. File selection remains materialized in the immutable plan and should normally be consumed through `file_args` rather than reparsed from Git state.

Human-readable output is intentionally compact. `plan --json`, `check --json`, and `list --json` provide the structured interfaces for automation and diagnostics.

There is no schema-v1 compatibility layer.
