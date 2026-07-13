# Migrating to schema v2

Schema v2 removes task-level profile execution mechanics. Recreate configuration around these rules:

1. Declare every logical task once.
2. Put commands and policy metadata under `implementations`.
3. Declare profiles at the document root.
4. Declare execution contexts at the document root.
5. Invoke both a profile and a context explicitly.

## Before

```json
{
  "version": 1,
  "node": {
    "tasks": [
      {
        "id": "typecheck",
        "phase": "verify",
        "kind": "command",
        "command": ["nix", "build", ".#typecheck"],
        "profiles": ["manual"]
      },
      {
        "id": "typecheck-direct",
        "phase": "verify",
        "kind": "command",
        "command": ["tsc", "--noEmit"],
        "profiles": ["nix-check"]
      }
    ]
  }
}
```

## After

```json
{
  "version": 2,
  "profiles": {
    "full": {
      "phase": "verify",
      "selection": "full",
      "tasks": ["typecheck"],
      "contexts": ["local", "nix-sandbox"]
    }
  },
  "contexts": {
    "local": {
      "implementation": "nix",
      "allow_network": true
    },
    "nix-sandbox": {
      "implementation": "direct",
      "require_sandbox_safe": true
    }
  },
  "node": {
    "id": "project",
    "tasks": [
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
    ]
  }
}
```

Commands are now explicit:

```sh
tend check --profile full --context local
tend check --profile full --context nix-sandbox
```
