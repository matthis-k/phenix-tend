# Tend architecture

Tend separates four concerns that would otherwise be mixed together.

## Logical tasks

A task defines **what outcome exists**: its identity, phase, change trigger, dependencies, and one or more implementations. A task is declared once.

```json
{
  "id": "typecheck",
  "phase": "verify",
  "implementations": {
    "nix": {
      "kind": "command",
      "command": ["nix", "build", ".#typecheck"],
      "network": true
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

Profiles do not contain commands or environment mechanics. A reusable profile such as `full` can run in both `local` and `nix-sandbox` contexts without duplicating its task membership.

## Execution contexts

An execution context defines **how execution is allowed to happen**. It chooses an implementation variant and states capability constraints such as mutation, interactivity, network access, and sandbox safety.

Contexts do not select tasks. Infrastructure restrictions therefore do not become pseudo-profiles.

## Implementations

An implementation is one concrete mechanism for a logical task. The planner selects the context's requested implementation variant, falling back to `default` when the task is mechanism-independent.

Policy is checked after implementation selection. A restrictive context therefore cannot accidentally execute a networked, interactive, mutating, or non-sandbox-safe implementation.

Command implementations may explicitly request selected files as arguments:

- `"file_args": "none"` appends nothing and is the default.
- `"file_args": "matched"` appends only files matching the task's `when.changed` paths.
- `"file_args": "selected"` appends the complete profile-selected file set.

The planner materializes these arguments in the immutable plan. Scripts do not parse Tend-specific environment payloads or reimplement Git selection.

## Configuration contract

`.tend.json` has one schema: the schema implemented by the checked-out Tend revision. It has no API-version discriminator and no compatibility parser for historical formats. Unknown root fields are rejected so stale configuration fails explicitly instead of being silently accepted.

Repositories coupled through lockfiles must update their Tend input and configuration together. Historical schemas remain available in Git history rather than in the runtime implementation.

## Command trust boundary

A `.tend.json` file is executable repository configuration, not untrusted data. A command implementation runs the configured executable directly, with the configured arguments, working directory, and environment. Tend does not invoke a shell unless the configuration explicitly names one.

Users and automation must therefore review Tend configuration with the same care as build scripts and CI workflows. Cloning or checking out an untrusted repository and running `tend check` can execute code from that repository.

Execution-context capability checks constrain declared behavior; they are not a security sandbox. Cross-repository composition must preserve an explicit trust decision at the repository boundary rather than silently importing executable task definitions.

The planner injects stable metadata variables into command environments: `TEND_PROFILE`, `TEND_CONTEXT`, `TEND_SELECTION`, and, when supplied, `TEND_BASE` and `TEND_HEAD`. They describe the already-resolved plan and do not replace selection or policy enforcement.

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
  -> file-argument materialization
  -> immutable execution plan
  -> execution
```

The plan is the boundary between policy and mechanism. Execution does not decide which tasks or implementations should run.
