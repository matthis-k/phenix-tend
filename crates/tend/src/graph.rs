use std::collections::{HashMap, HashSet};

use crate::model::{TaskConfig, TendError};

pub fn order_tasks(
    roots: &[String],
    tasks: &HashMap<&str, &TaskConfig>,
) -> Result<Vec<String>, TendError> {
    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    for root in roots {
        visit(root, tasks, &mut visiting, &mut visited, &mut ordered)?;
    }

    Ok(ordered)
}

fn visit(
    task_id: &str,
    tasks: &HashMap<&str, &TaskConfig>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) -> Result<(), TendError> {
    if visited.contains(task_id) {
        return Ok(());
    }
    if !visiting.insert(task_id.to_string()) {
        return Err(TendError::DependencyCycle(task_id.to_string()));
    }

    let task = tasks
        .get(task_id)
        .copied()
        .ok_or_else(|| TendError::UnknownTask(task_id.to_string()))?;
    for dependency in &task.requires {
        visit(dependency, tasks, visiting, visited, ordered)?;
    }

    visiting.remove(task_id);
    visited.insert(task_id.to_string());
    ordered.push(task_id.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{Phase, TaskImplementation, TaskKind};

    fn task(id: &str, requires: &[&str]) -> TaskConfig {
        TaskConfig {
            id: id.to_string(),
            description: None,
            phase: Phase::Verify,
            tags: Vec::new(),
            when: None,
            always: false,
            requires: requires.iter().map(|value| (*value).to_string()).collect(),
            implementations: BTreeMap::from([(
                "default".to_string(),
                TaskImplementation {
                    kind: TaskKind::Command {
                        command: vec!["true".to_string()],
                        expect_status: 0,
                    },
                    mutates: false,
                    interactive: false,
                    network: false,
                    sandbox_safe: true,
                    env: BTreeMap::new(),
                    workdir: None,
                },
            )]),
        }
    }

    #[test]
    fn dependencies_precede_consumers() {
        let tasks = [task("compile", &["generate"]), task("generate", &[])];
        let lookup = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
        let ordered = order_tasks(&["compile".to_string()], &lookup).expect("order");
        assert_eq!(ordered, vec!["generate", "compile"]);
    }

    #[test]
    fn cycles_are_rejected() {
        let tasks = [task("a", &["b"]), task("b", &["a"])];
        let lookup = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
        let error = order_tasks(&["a".to_string()], &lookup).expect_err("cycle");
        assert!(matches!(error, TendError::DependencyCycle(_)));
    }
}
