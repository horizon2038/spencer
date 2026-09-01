mod cli;
mod steps;

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let repo_root = std::env::current_dir().context("get current_dir")?;
    let repo_root = Utf8PathBuf::from_path_buf(repo_root)
        .map_err(|_| anyhow::anyhow!("repo root path is not valid utf-8"))?;

    match cli.command {
        cli::Command::Build(args) => {
            run_build_pipeline(&repo_root, &args.common, args.enable_smp)?;
        }
        cli::Command::Run(args) => {
            if args.common.platform != cli::Platform::Qemu {
                bail!(
                    "cargo xtask run supports only --platform qemu; build the rpi4b image and write it to removable media"
                );
            }
            if args.smp.get() > 1 && !args.enable_smp {
                bail!("--smp greater than 1 requires --enable-smp");
            }
            run_build_pipeline(&repo_root, &args.common, args.enable_smp)?;
            run_qemu(&repo_root, &args)?;
        }
    }

    Ok(())
}

fn run_build_pipeline(
    repo_root: &camino::Utf8Path,
    common: &cli::CommonArgs,
    enable_smp: bool,
) -> Result<()> {
    let kernel_args = steps::kernel::BuildKernelArgs {
        arch: common.arch.clone(),
        platform: common.platform.clone(),
        release: common.release,
        enable_smp,
        verbose: common.verbose,
        dry_run: common.dry_run,
    };

    steps::kernel::build_kernel(repo_root, &kernel_args)?;

    let nun_os_args = steps::nun::BuildNunOsArgs {
        arch: common.arch.clone(),
        platform: common.platform.clone(),
        release: common.release,
        verbose: common.verbose,
        dry_run: common.dry_run,
        use_nightly_build_std: true,
        os_manifest: common.os_manifest.clone(),
        os_target_json: common.os_target_json.clone(),
        os_binary: common.os_binary.clone(),
    };

    let nun_os_artifacts = steps::nun::build_nun_os(repo_root, &nun_os_args)?;

    let target_arch = match kernel_args.arch {
        cli::Arch::X86_64 => "x86_64",
        cli::Arch::Aarch64 => "aarch64",
        cli::Arch::Riscv64 => "riscv64",
    };

    let platform_name = match kernel_args.platform {
        cli::Platform::Qemu => "qemu",
        cli::Platform::Rpi4b => "rpi4b",
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

    let init_elf_source = nun_os_artifacts.executable_path;
    let kernel_elf_source = out_base.join("a9n").join("kernel.elf");
    match &common.arch {
        cli::Arch::X86_64 => {
            let a9nloader_args = steps::a9nloader::BuildA9nloaderArgs {
                arch: common.arch.clone(),
                platform: common.platform.clone(),
                release: common.release,
                verbose: common.verbose,
                dry_run: common.dry_run,
            };
            let a9nloader_artifacts =
                steps::a9nloader::build_a9nloader(repo_root, &a9nloader_args)?;
            let bootx64_efi_source = a9nloader_artifacts.out_dir.join("a9nloader-rs.efi");
            steps::image::build_fat_img(&steps::image::BuildImgArgs {
                img_path: &img_path,
                bootx64_efi_source_path: &bootx64_efi_source,
                init_elf_source_path: &init_elf_source,
                kernel_elf_source_path: &kernel_elf_source,
                image_size_mib: 64,
                verbose: kernel_args.verbose,
                dry_run: kernel_args.dry_run,
            })?;
        }
        cli::Arch::Aarch64 => {
            let uboot_artifacts = steps::uboot::build_uboot(
                repo_root,
                &steps::uboot::BuildUbootArgs {
                    arch: common.arch.clone(),
                    platform: common.platform.clone(),
                    out_base: &out_base,
                    verbose: kernel_args.verbose,
                    dry_run: kernel_args.dry_run,
                },
            )?;
            let kernel_image_source = out_base.join("a9n").join("kernel.img");
            match common.platform {
                cli::Platform::Qemu => {
                    steps::image::build_uboot_fat_img(&steps::image::BuildUbootImgArgs {
                        img_path: &img_path,
                        uboot_binary_source_path: &uboot_artifacts.binary_path,
                        init_elf_source_path: &init_elf_source,
                        kernel_image_source_path: &kernel_image_source,
                        image_size_mib: 64,
                        verbose: kernel_args.verbose,
                        dry_run: kernel_args.dry_run,
                    })?;
                }
                cli::Platform::Rpi4b => {
                    let firmware_artifacts = steps::rpi_firmware::build_rpi_firmware(
                        repo_root,
                        &steps::rpi_firmware::BuildRpiFirmwareArgs {
                            out_base: &out_base,
                            verbose: kernel_args.verbose,
                            dry_run: kernel_args.dry_run,
                        },
                    )?;
                    steps::image::build_rpi4b_img(&steps::image::BuildRpi4bImgArgs {
                        img_path: &img_path,
                        firmware_boot_dir: &firmware_artifacts.boot_dir,
                        uboot_binary_source_path: &uboot_artifacts.binary_path,
                        init_elf_source_path: &init_elf_source,
                        kernel_image_source_path: &kernel_image_source,
                        image_size_mib: 64,
                        verbose: kernel_args.verbose,
                        dry_run: kernel_args.dry_run,
                    })?;
                }
            }
        }
        cli::Arch::Riscv64 => bail!("riscv64 image construction is not implemented"),
    }

    Ok(())
}

fn run_qemu(repo_root: &camino::Utf8Path, args: &cli::RunArgs) -> Result<()> {
    if args.common.platform != cli::Platform::Qemu {
        bail!("QEMU execution requires --platform qemu");
    }
    let target_arch = match args.common.arch {
        cli::Arch::X86_64 => "x86_64",
        cli::Arch::Aarch64 => "aarch64",
        cli::Arch::Riscv64 => "riscv64",
    };

    let platform_name = match args.common.platform {
        cli::Platform::Qemu => "qemu",
        cli::Platform::Rpi4b => unreachable!("rpi4b was rejected above"),
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
    let uboot_binary_path = out_base.join("u-boot").join("u-boot.bin");

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
        uboot_binary_path: &uboot_binary_path,
        enable_gdb: args.gdb,
        stop_at_start: args.stop,
        smp: args.smp.get(),
        accel: args.accel,
        verbose: args.common.verbose,
        dry_run: args.common.dry_run,
    };

    match args.common.arch {
        cli::Arch::X86_64 => steps::qemu::run_qemu_x86_64(&qemu_args)?,
        cli::Arch::Aarch64 => steps::qemu::run_qemu_aarch64(&qemu_args)?,
        cli::Arch::Riscv64 => bail!("riscv64 QEMU execution is not implemented"),
    }

    Ok(())
}
