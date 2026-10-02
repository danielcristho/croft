//! The docker and kubectl CLIs that deploy and collect call.

use std::{
    fs::File,
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, bail};

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

pub fn output(program: &str, args: &[String]) -> anyhow::Result<String> {
    let output = Command::new(program)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("start {program}"))?;
    check(program, args, output.status.success())?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn to_file(program: &str, args: &[String], file: File) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdout(file)
        .status()
        .with_context(|| format!("start {program}"))?;
    check(program, args, status.success())
}

pub fn display(program: &str, args: &[String]) -> String {
    format!("{program} {}", args.join(" "))
}

fn check(program: &str, args: &[String], success: bool) -> anyhow::Result<()> {
    if !success {
        bail!("run {}", display(program, args));
    }
    Ok(())
}
