use anyhow::{Context, Result, bail};
use std::process::Command;

pub fn run_command(mut command: Command, verbose: bool, context: &str) -> Result<()> {
    if verbose {
        eprintln!("[cmd] {:?}", command);
    }

    let status = command
        .status()
        .with_context(|| format!("failed to spawn: {}", context))?;

    if !status.success() {
        bail!("command failed: {} (exit={})", context, status);
    }

    Ok(())
}
