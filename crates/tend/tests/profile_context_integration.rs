use std::fs;

use tend::{PlanRequest, TaskKind, TaskStatus};

fn write_config(root: &std::path::Path, networked_direct: bool) {
    let direct_network = if networked_direct { "true" } else { "false" };
    let config = format!(
        r#"{{
  "profiles": {{
    "verify": {{
      "phase": "verify",
      "selection": "changed",
      "tasks": ["typecheck"],
      "contexts": ["local", "sandbox"]
    }}
  }},
  "contexts": {{
    "local": {{
      "implementation": "nix",
      "allow_network": true
    }},
    "sandbox": {{
      "implementation": "direct",
      "require_sandbox_safe": true
    }}
  }},
  "node": {{
    "id": "test-project",
    "tasks": [
      {{
        "id": "typecheck",
        "phase": "verify",
        "when": {{
          "changed": {{
            "paths": ["src/**/*.rs"]
          }}
        }},
        "implementations": {{
          "nix": {{
            "kind": "command",
            "command": ["sh", "-c", "printf nix"],
            "network": true
          }},
          "direct": {{
            "kind": "command",
            "command": ["sh", "-c", "printf direct"],
            "network": {direct_network},
            "sandbox_safe": true
          }}
        }}
      }}
    ]
  }}
}}"#
    );
    fs::write(root.join(".tend.json"), config).expect("write Tend configuration");
}

#[test]
fn profile_and_context_resolve_one_logical_task() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_config(directory.path(), false);

    let workspace = tend::load(directory.path()).expect("load configuration");
    let plan = tend::plan(
        &workspace,
        &PlanRequest {
            profile: "verify".to_string(),
            context: "sandbox".to_string(),
            base: None,
            head: None,
            files: Some(vec!["src/lib.rs".to_string()]),
        },
    )
    .expect("resolve plan");

    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].task_id, "typecheck");
    assert_eq!(plan.items[0].implementation, "direct");
    match &plan.items[0].kind {
        TaskKind::Command { command, .. } => assert_eq!(command[2], "printf direct"),
        _ => panic!("expected command implementation"),
    }

    let results = tend::execute(&plan);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, TaskStatus::Passed);
    assert_eq!(results[0].stdout, "direct");
}

#[test]
fn load_rejects_an_unsafe_profile_context_matrix() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_config(directory.path(), true);

    let error = tend::load(directory.path()).expect_err("networked sandbox task must fail");
    assert!(error.to_string().contains("network access is not allowed"));
}

#[test]
fn load_rejects_a_legacy_version_discriminator() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_config(directory.path(), false);

    let path = directory.path().join(".tend.json");
    let config = fs::read_to_string(&path).expect("read Tend configuration");
    let versioned = config.replacen('{', "{\n  \"version\": 2,", 1);
    fs::write(&path, versioned).expect("write legacy Tend configuration");

    let error = tend::load(directory.path()).expect_err("legacy version field must fail");
    assert!(error.to_string().contains("unknown field `version`"));
}
