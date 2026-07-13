# Tend v2 architecture

Tend separates four concerns that were previously mixed together.

## Logical tasks

A task defines **what outcome exists**: its identity, phase, change trigger, dependencies, and one or more implementations. A task is declared once.

```json
{
  "id": "typecheck",
  "phase": "verify",
  "implementations": {
    "nix": {
      "kind": "command",
      "command": ["nix", "build", ".#typecheck"]
    },
    "direct": {
      "kind": "command",
      "command": ["tsc", "--noEmit"],
      "sandbox_safe": true
    }
  }
}
```

## Profiles

A profile defines **what should run**. It owns the phase, change-selection strategy, ordered task set, and the execution contexts in which that task set is valid.

Profiles do not contain commands or environment mechanics.

## Execution contexts

An execution context defines **how execution is allowed to happen**. It chooses an implementation variant and states capability constraints such as mutation, interactivity, network access, and sandbox safety.

Contexts do not select tasks.

## Implementations

An implementation is one concrete mechanism for a logical task. The planner selects the context's requested implementation variant, falling back to `default` when the task is mechanism-independent.

Policy is checked after implementation selection. A restrictive context therefore cannot accidentally execute a networked, interactive, mutating, or non-sandbox-safe implementation.

## Change selection

Change selection is profile data:

- `changed`: worktree, index, and untracked files
- `staged`: index only
- `full`: every selected task
- `git-range`: the exact `--base` / `--head` revision range

CI only supplies revision metadata. It does not reproduce Tend's selection logic.

## Planning pipeline

```text
configuration
  -> schema and cross-reference validation
  -> profile resolution
  -> execution-context resolution
  -> changed-file selection
  -> logical task selection
  -> dependency expansion and cycle detection
  -> implementation selection
  -> capability-policy validation
  -> immutable execution plan
  -> execution
```

The plan is the boundary between policy and mechanism. Execution does not decide which tasks or implementations should run.

## Deliberate break

Schema v1 and the previous CLI are not supported. This release is an architectural reset while Tend is still pre-stable. Repositories must migrate their `.tend.json` and invoke commands in the explicit form:

```sh
tend check --profile ci --context local --base "$BASE" --head "$HEAD"
```
