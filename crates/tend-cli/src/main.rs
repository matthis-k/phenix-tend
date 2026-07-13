use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tend::{PlanRequest, TaskKind, TaskStatus};

#[derive(Parser)]
#[command(
    name = "tend",
    version,
    about = "Deterministic profile and execution-context task runner"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate the complete configuration, profile, context, and implementation matrix.
    Validate,

    /// Show the resolved task plan without executing it.
    Plan {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        context: String,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        head: Option<String>,
        #[arg(long = "file")]
        files: Vec<String>,
        #[arg(long)]
        json: bool,
    },

    /// Resolve and execute one profile in one execution context.
    Check {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        context: String,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        head: Option<String>,
        #[arg(long = "file")]
        files: Vec<String>,
        #[arg(long)]
        json: bool,
    },

    /// List profiles, execution contexts, and logical tasks.
    List {
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
    }
}

fn run(cli: Cli) -> Result<i32, tend::TendError> {
    let workspace = tend::load(&cli.root)?;

    match cli.command {
        Command::Validate => {
            tend::validate(&workspace)?;
            println!(
                "configuration valid: {} profile(s), {} context(s), {} task(s)",
                workspace.config.profiles.len(),
                workspace.config.contexts.len(),
                workspace.config.node.tasks.len()
            );
            Ok(0)
        }
        Command::Plan {
            profile,
            context,
            base,
            head,
            files,
            json,
        } => {
            let plan = tend::plan(
                &workspace,
                &request(profile, context, base, head, files),
            )?;
            print_plan(&plan, json);
            Ok(0)
        }
        Command::Check {
            profile,
            context,
            base,
            head,
            files,
            json,
        } => {
            let plan = tend::plan(
                &workspace,
                &request(profile, context, base, head, files),
            )?;
            if !json {
                print_plan(&plan, false);
                println!();
            }
            let results = tend::execute(&plan);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&results).expect("serialize results")
                );
            } else {
                print_results(&results);
            }
            Ok(if tend::has_failures(&results) { 1 } else { 0 })
        }
        Command::List { json } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&workspace.config).expect("serialize config")
                );
            } else {
                print_workspace(&workspace);
            }
            Ok(0)
        }
    }
}

fn request(
    profile: String,
    context: String,
    base: Option<String>,
    head: Option<String>,
    files: Vec<String>,
) -> PlanRequest {
    PlanRequest {
        profile,
        context,
        base,
        head,
        files: (!files.is_empty()).then_some(files),
    }
}

fn print_workspace(workspace: &tend::Workspace) {
    println!("{}", workspace.config.node.id);
    if let Some(description) = &workspace.config.node.description {
        println!("  {description}");
    }

    println!("\nProfiles:");
    for (name, profile) in &workspace.config.profiles {
        println!(
            "  {name}: phase={}, selection={}, contexts=[{}], tasks=[{}]",
            profile.phase,
            profile.selection,
            profile.contexts.join(", "),
            profile.tasks.join(", ")
        );
    }

    println!("\nExecution contexts:");
    for (name, context) in &workspace.config.contexts {
        println!(
            "  {name}: implementation={}, mutation={}, interactive={}, network={}, require_sandbox_safe={}",
            context.implementation,
            context.allow_mutation,
            context.allow_interactive,
            context.allow_network,
            context.require_sandbox_safe
        );
    }

    println!("\nTasks:");
    for task in &workspace.config.node.tasks {
        println!(
            "  {}: phase={}, implementations=[{}]",
            task.id,
            task.phase,
            task.implementations
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn print_plan(plan: &tend::Plan, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(plan).expect("serialize plan")
        );
        return;
    }

    println!(
        "Profile '{}' in context '{}' ({}, {} selected file(s)):",
        plan.profile,
        plan.context,
        plan.selection,
        plan.files.len()
    );
    if plan.items.is_empty() {
        println!("  no tasks selected");
        return;
    }
    for item in &plan.items {
        let command = match &item.kind {
            TaskKind::Command { command, .. } => command.join(" "),
            kind => kind.name().to_string(),
        };
        println!(
            "  {} [{}; {:?}] {}",
            item.task_id, item.implementation, item.reason, command
        );
    }
}

fn print_results(results: &[tend::TaskResult]) {
    for result in results {
        let label = match result.status {
            TaskStatus::Passed => "PASS",
            TaskStatus::Failed => "FAIL",
            TaskStatus::Skipped => "SKIP",
        };
        println!("[{label}] {}", result.task_id);
        if !result.stdout.trim().is_empty() {
            print!("{}", result.stdout);
            if !result.stdout.ends_with('\n') {
                println!();
            }
        }
        if !result.stderr.trim().is_empty() {
            eprint!("{}", result.stderr);
            if !result.stderr.ends_with('\n') {
                eprintln!();
            }
        }
    }
}
