//! Workspace automation commands.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let command = std::env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "check" => {
            run_cargo(&["fmt", "--all", "--", "--check"])?;
            run_cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])?;
            run_cargo(&["test", "--workspace"])
        }
        "fmt" => run_cargo(&["fmt", "--all"]),
        "clippy" => run_cargo(&[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
        "test" => run_cargo(&["test", "--workspace"]),
        "wasm-check" => run_cargo(&[
            "check",
            "-p",
            "sim-core",
            "-p",
            "editor-core",
            "-p",
            "worker-protocol",
            "-p",
            "worker",
            "-p",
            "circuitsim-app",
            "--target",
            "wasm32-unknown-unknown",
        ]),
        "web-build" => run_tool("trunk", &["build", "--release"]),
        "web-serve" => run_tool("trunk", &["serve"]),
        "help" | "--help" | "-h" => {
            println!(
                "usage: cargo xtask <command>\n\ncommands:\n  check       fmt check, clippy, and tests\n  fmt         format the workspace\n  clippy      run clippy with warnings denied\n  test        run workspace tests\n  wasm-check  check libraries, the worker, and the app for wasm32-unknown-unknown\n  web-build   build the web distribution with Trunk\n  web-serve   serve the web application with Trunk"
            );
            Ok(())
        }
        other => Err(format!("unknown command '{other}'; run `cargo xtask help`")),
    }
}

fn run_cargo(arguments: &[&str]) -> Result<(), String> {
    run_tool("cargo", arguments)
}

fn run_tool(program: &str, arguments: &[&str]) -> Result<(), String> {
    println!("{program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| format!("failed to start {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {} failed with {status}",
            arguments.join(" ")
        ))
    }
}
