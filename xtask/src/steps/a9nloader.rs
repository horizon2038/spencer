use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct BuildA9nloaderArgs {
    pub arch: Arch,
    pub platform: Platform,
    pub release: bool,
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn build_a9nloader(
    repo_root: &Utf8Path,
    args: &BuildA9nloaderArgs,
) -> Result<A9nloaderArtifacts> {
    validate_supported(&args.arch, &args.platform)?;

    let a9nloader_dir = repo_root.join("a9nloader-rs");

    let profile_dir_name = if args.release { "release" } else { "debug" };

    let cargo_target = match to_cargo_target_triple(&args.arch) {
        Some(value) => value,
        None => {
            bail!(
                "unsupported arch for a9nloader cargo target: {:?}",
                args.arch
            );
        }
    };

    let out_dir = repo_root
        .join("out")
        .join(format!(
            "{}-{}-{}",
            to_arch_name(&args.arch),
            to_platform_name(&args.platform),
            if args.release { "release" } else { "debug" }
        ))
        .join("a9nloader");

    let produced_dir = a9nloader_dir
        .join("target")
        .join(cargo_target)
        .join(profile_dir_name);

    if args.dry_run {
        eprintln!("[dry-run] cargo build (A9NLoader)");
        eprintln!("[dry-run]   dir: {}", a9nloader_dir);
        eprintln!("[dry-run]   --target {}", cargo_target);
        if args.release {
            eprintln!("[dry-run]   --release");
        }
        eprintln!("[dry-run] produced_dir: {}", produced_dir);
        eprintln!("[dry-run] out_dir: {}", out_dir);
        return Ok(A9nloaderArtifacts {
            out_dir,
            produced_dir,
        });
    }

    std::fs::create_dir_all(&out_dir).with_context(|| format!("create out dir: {}", out_dir))?;

    let mut build_command = Command::new("cargo");
    build_command.current_dir(&a9nloader_dir);
    build_command.arg("build");
    build_command.arg("--target");
    build_command.arg(cargo_target);

    if args.release {
        build_command.arg("--release");
    }

    run_command(build_command, args.verbose, "cargo build (A9NLoader)")?;

    copy_dir_contents(&produced_dir, &out_dir)
        .with_context(|| format!("copy a9nloader artifacts: {} -> {}", produced_dir, out_dir))?;

    Ok(A9nloaderArtifacts {
        out_dir,
        produced_dir,
    })
}

#[derive(Clone, Debug)]
pub struct A9nloaderArtifacts {
    pub out_dir: Utf8PathBuf,
    pub produced_dir: Utf8PathBuf,
}

fn validate_supported(_arch: &Arch, _platform: &Platform) -> Result<()> {
    Ok(())
}

fn to_arch_name(arch: &Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "x86_64",
        Arch::Aarch64 => "aarch64",
        Arch::Riscv64 => "riscv64",
    }
}

fn to_platform_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Qemu => "qemu",
    }
}

fn to_cargo_target_triple(arch: &Arch) -> Option<&'static str> {
    match arch {
        Arch::X86_64 => Some("x86_64-unknown-uefi"),
        Arch::Aarch64 => Some("aarch64-unknown-uefi"),
        Arch::Riscv64 => None,
    }
}

fn copy_dir_contents(source_dir: &Utf8Path, destination_dir: &Utf8Path) -> Result<()> {
    if !source_dir.exists() {
        bail!("source_dir does not exist: {}", source_dir);
    }

    for entry in std::fs::read_dir(source_dir.as_std_path())
        .with_context(|| format!("read_dir: {}", source_dir))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let file_name = entry.file_name();

        let file_name = file_name
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-utf8 file name in {}", source_dir))?;

        let src_path = source_dir.join(file_name);
        let dst_path = destination_dir.join(file_name);

        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path)
                .with_context(|| format!("create_dir_all: {}", dst_path))?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy: {} -> {}", src_path, dst_path))?;
        }
    }

    Ok(())
}
