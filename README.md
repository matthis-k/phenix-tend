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

There is no schema-v1 compatibility layer.
