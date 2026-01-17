mod cli;
mod steps;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let repo_root = std::env::current_dir().context("get current_dir")?;
    let repo_root = Utf8PathBuf::from_path_buf(repo_root)
        .map_err(|_| anyhow::anyhow!("repo root path is not valid utf-8"))?;

    match cli.command {
        cli::Command::Build(args) => {
            run_build_pipeline(&repo_root, &args.common)?;
        }
        cli::Command::Run(args) => {
            run_build_pipeline(&repo_root, &args.common)?;
            run_qemu(&repo_root, &args)?;
        }
    }

    Ok(())
}

fn run_build_pipeline(repo_root: &camino::Utf8Path, common: &cli::CommonArgs) -> Result<()> {
    let kernel_args = steps::kernel::BuildKernelArgs {
        arch: common.arch.clone(),
        platform: common.platform.clone(),
        release: common.release,
        verbose: common.verbose,
        dry_run: common.dry_run,
    };

    steps::kernel::build_kernel(repo_root, &kernel_args)?;

    let a9nloader_args = steps::a9nloader::BuildA9nloaderArgs {
        arch: common.arch.clone(),
        platform: common.platform.clone(),
        release: common.release,
        verbose: common.verbose,
        dry_run: common.dry_run,
    };

    steps::a9nloader::build_a9nloader(repo_root, &a9nloader_args)?;

    let nun_os_args = steps::nun::BuildNunOsArgs {
        arch: common.arch.clone(),
        platform: common.platform.clone(),
        release: common.release,
        verbose: common.verbose,
        dry_run: common.dry_run,
        use_nightly_build_std: true,
    };

    steps::nun::build_nun_os(repo_root, &nun_os_args)?;

    let target_arch = match kernel_args.arch {
        cli::Arch::X86_64 => "x86_64",
        cli::Arch::Aarch64 => "aarch64",
        cli::Arch::Riscv64 => "riscv64",
    };

    let platform_name = match kernel_args.platform {
        cli::Platform::Qemu => "qemu",
    };

    let out_base = repo_root.join("out").join(format!(
        "{}-{}-{}",
        target_arch,
        platform_name,
        if kernel_args.release {
            "release"
        } else {
            "debug"
        },
    ));

    let img_path = out_base.join("spencer.img");

    // Sources are currently x86_64-fixed based on your confirmed output paths.
    // Generalize later by deriving from the target json or a mapping table.
    let bootx64_efi_source = out_base.join("a9nloader").join("a9nloader-rs.efi");

    let init_elf_source = out_base
        .join("nun_os_target_dir")
        .join("x86_64-unknown-a9n")
        .join(if kernel_args.release {
            "release"
        } else {
            "debug"
        })
        .join("core");

    let kernel_elf_source = out_base.join("a9n").join("kernel.elf");

    let img_args = steps::image::BuildImgArgs {
        img_path: &img_path,
        bootx64_efi_source_path: &bootx64_efi_source,
        init_elf_source_path: &init_elf_source,
        kernel_elf_source_path: &kernel_elf_source,
        image_size_mib: 64,
        verbose: kernel_args.verbose,
        dry_run: kernel_args.dry_run,
    };

    steps::image::build_fat_img(&img_args)?;

    Ok(())
}

fn run_qemu(repo_root: &camino::Utf8Path, args: &cli::RunArgs) -> Result<()> {
    let target_arch = match args.common.arch {
        cli::Arch::X86_64 => "x86_64",
        cli::Arch::Aarch64 => "aarch64",
        cli::Arch::Riscv64 => "riscv64",
    };

    let platform_name = match args.common.platform {
        cli::Platform::Qemu => "qemu",
    };

    let out_base = repo_root.join("out").join(format!(
        "{}-{}-{}",
        target_arch,
        platform_name,
        if args.common.release {
            "release"
        } else {
            "debug"
        },
    ));

    let img_path = out_base.join("spencer.img");

    // OVMF paths (A9NLoader tools)
    let ovmf_code_path = repo_root
        .join("a9nloader-rs")
        .join("tools")
        .join("OVMF_CODE.fd");

    let ovmf_vars_path = repo_root
        .join("a9nloader-rs")
        .join("tools")
        .join("OVMF_VARS.fd");

    let qemu_args = steps::qemu::RunQemuArgs {
        arch: args.common.arch.clone(),
        platform: args.common.platform.clone(),
        out_base: &out_base,
        img_path: &img_path,
        ovmf_code_path: &ovmf_code_path,
        ovmf_vars_path: &ovmf_vars_path,
        enable_gdb: args.gdb,
        stop_at_start: args.stop,
        verbose: args.common.verbose,
        dry_run: args.common.dry_run,
    };

    steps::qemu::run_qemu_x86_64(&qemu_args)?;

    Ok(())
}
