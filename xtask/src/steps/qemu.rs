use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct RunQemuArgs<'a> {
    pub arch: Arch,
    pub platform: Platform,
    pub out_base: &'a Utf8Path,

    pub img_path: &'a Utf8Path,

    pub ovmf_code_path: &'a Utf8Path,
    pub ovmf_vars_path: &'a Utf8Path,

    pub enable_gdb: bool,
    pub stop_at_start: bool,

    pub verbose: bool,
    pub dry_run: bool,
}

pub fn run_qemu_x86_64(args: &RunQemuArgs) -> Result<()> {
    if args.arch != Arch::X86_64 {
        bail!("run_qemu_x86_64 called with non-x86_64 arch");
    }

    if args.platform != Platform::Qemu {
        bail!("run_qemu_x86_64 called with non-qemu platform");
    }

    if args.dry_run {
        eprintln!("[dry-run] qemu-system-x86_64 ...");
        eprintln!("[dry-run]   img: {}", args.img_path);
        eprintln!("[dry-run]   OVMF_CODE: {}", args.ovmf_code_path);
        eprintln!("[dry-run]   OVMF_VARS: {}", args.ovmf_vars_path);
        return Ok(());
    }

    let ovmf_vars_runtime = args.out_base.join("OVMF_VARS.fd");
    std::fs::copy(
        args.ovmf_vars_path.as_std_path(),
        ovmf_vars_runtime.as_std_path(),
    )
    .with_context(|| {
        format!(
            "copy OVMF_VARS: {} -> {}",
            args.ovmf_vars_path, ovmf_vars_runtime
        )
    })?;

    let mut command = Command::new("qemu-system-x86_64");
    command.arg("-m").arg("4G");
    command.arg("-cpu").arg("max");
    command.arg("-net").arg("none");
    command.arg("-serial").arg("mon:stdio");

    command.arg("-drive").arg(format!(
        "if=pflash,format=raw,readonly=on,file={}",
        args.ovmf_code_path
    ));

    command
        .arg("-drive")
        .arg(format!("if=pflash,format=raw,file={}", ovmf_vars_runtime));

    command
        .arg("-drive")
        .arg(format!("format=raw,file={}", args.img_path));

    command
        .arg("-netdev")
        .arg("user,id=net0,hostfwd=tcp:127.0.0.1:1234-:80");
    command.arg("-device").arg("e1000,netdev=net0");

    if args.enable_gdb {
        command.arg("-s");
    }
    if args.stop_at_start {
        command.arg("-S");
    }

    run_command(command, args.verbose, "qemu-system-x86_64")?;

    Ok(())
}
