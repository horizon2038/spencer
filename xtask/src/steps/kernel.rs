use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result};
use camino::Utf8Path;
use std::process::Command;

#[derive(Clone, Debug)]
pub struct BuildKernelArgs {
    pub arch: Arch,
    pub platform: Platform,
    pub release: bool,
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn build_kernel(repo_root: &Utf8Path, args: &BuildKernelArgs) -> Result<()> {
    validate_supported(&args.arch, &args.platform)?;

    let target_arch = to_a9n_target_arch(&args.arch);
    let platform_name = to_platform_name(&args.platform);
    let build_type = if args.release { "Release" } else { "Debug" };

    let a9n_dir = repo_root.join("A9N");

    let build_dir = a9n_dir.join("build").join(format!(
        "{}-{}-{}",
        target_arch,
        platform_name,
        if args.release { "release" } else { "debug" }
    ));

    let install_prefix = repo_root
        .join("out")
        .join(format!(
            "{}-{}-{}",
            target_arch,
            platform_name,
            if args.release { "release" } else { "debug" }
        ))
        .join("a9n");

    let toolchain_file = a9n_dir
        .join("src")
        .join("hal")
        .join(target_arch)
        .join("toolchain.cmake");

    if args.dry_run {
        eprintln!("[dry-run] cmake -S {} -B {}", a9n_dir, build_dir);
        eprintln!("[dry-run]   -DARCH={}", target_arch);
        eprintln!("[dry-run]   -DCMAKE_TOOLCHAIN_FILE={}", toolchain_file);
        eprintln!("[dry-run]   -DCMAKE_BUILD_TYPE={}", build_type);
        eprintln!("[dry-run]   -DCMAKE_INSTALL_PREFIX={}", install_prefix);
        eprintln!("[dry-run] cmake --build {}", build_dir);
        eprintln!("[dry-run] cmake --install {}", build_dir);
        return Ok(());
    }

    std::fs::create_dir_all(&build_dir)
        .with_context(|| format!("create build dir: {}", build_dir))?;
    std::fs::create_dir_all(&install_prefix)
        .with_context(|| format!("create install prefix: {}", install_prefix))?;

    let mut configure_command = Command::new("cmake");
    configure_command
        .current_dir(&a9n_dir)
        .arg("-S")
        .arg(".")
        .arg("-B")
        .arg(&build_dir)
        .arg(format!("-DARCH={}", target_arch))
        .arg(format!("-DCMAKE_TOOLCHAIN_FILE={}", toolchain_file))
        .arg(format!("-DCMAKE_BUILD_TYPE={}", build_type))
        .arg(format!("-DCMAKE_INSTALL_PREFIX={}", install_prefix));
    run_command(
        configure_command,
        args.verbose,
        "cmake configure (A9N kernel)",
    )?;

    let mut build_command = Command::new("cmake");
    build_command
        .current_dir(&a9n_dir)
        .arg("--build")
        .arg(&build_dir);
    run_command(build_command, args.verbose, "cmake build (A9N kernel)")?;

    let mut install_command = Command::new("cmake");
    install_command
        .current_dir(&a9n_dir)
        .arg("--install")
        .arg(&build_dir);
    run_command(install_command, args.verbose, "cmake install (A9N kernel)")?;

    eprintln!("installed to: {}", install_prefix);
    Ok(())
}

fn validate_supported(_arch: &Arch, _platform: &Platform) -> Result<()> {
    Ok(())
}

fn to_a9n_target_arch(arch: &Arch) -> &'static str {
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
