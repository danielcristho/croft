//! The docker and kubectl CLIs that deploy and collect call.

use std::{
    fs::File,
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, bail};

// run executes the given program with the given arguments, optionally writing the given stdin to its stdin, and returns an error if it fails.
pub fn run(program: &str, args: &[String], stdin: Option<&str>) -> anyhow::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .spawn()
        .with_context(|| format!("start {program}"))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())?;
    }
    check(program, args, child.wait()?.success())
}

// output executes the given program with the given arguments, and returns its stdout as a String, or an error if it fails.
pub fn output(program: &str, args: &[String]) -> anyhow::Result<String> {
    let output = Command::new(program)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("start {program}"))?;
    check(program, args, output.status.success())?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// to_file executes the given program with the given arguments, writing its stdout to the given file, and returns an error if it fails.
pub fn to_file(program: &str, args: &[String], file: File) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdout(file)
        .status()
        .with_context(|| format!("start {program}"))?;
    check(program, args, status.success())
}

// display returns a string that shows the program and its arguments, for logging or error messages.
pub fn display(program: &str, args: &[String]) -> String {
    format!("{program} {}", args.join(" "))
}

// check checks the success of a command, and returns an error if it failed.
fn check(program: &str, args: &[String], success: bool) -> anyhow::Result<()> {
    if !success {
        bail!("run {}", display(program, args));
    }
    Ok(())
}
