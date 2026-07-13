# Breaking v2 release

This branch intentionally removes schema-v1 and CLI compatibility.

Key changes:

- profiles are root-level task-selection data
- execution contexts are root-level mechanism and capability data
- logical tasks contain named implementations
- a context selects one implementation variant with `default` fallback
- profile/context compatibility is validated before planning
- native `changed`, `staged`, `full`, and `git-range` selection
- `check` and `plan` require both `--profile` and `--context`
- hard-coded semantics for names such as `nix-check` and `git-hook` are removed
- CI passes revision metadata to Tend instead of reproducing changed-file logic

The old MCP adapter is removed from the build while the core API is reset. A new adapter should consume immutable v2 plans rather than call planner internals directly.
