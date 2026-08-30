use crate::cli::{Arch, HardwareAcceleration, Platform};
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
    pub uboot_binary_path: &'a Utf8Path,

    pub enable_gdb: bool,
    pub stop_at_start: bool,
    pub smp: u16,
    pub accel: HardwareAcceleration,

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
    validate_smp(args.smp)?;

    if args.dry_run {
        eprintln!("[dry-run] qemu-system-x86_64 ...");
        eprintln!("[dry-run]   smp: {}", args.smp);
        eprintln!(
            "[dry-run]   hardware acceleration: {}",
            acceleration_policy_name(args.accel)
        );
        eprintln!("[dry-run]   img: {}", args.img_path);
        eprintln!("[dry-run]   OVMF_CODE: {}", args.ovmf_code_path);
        eprintln!("[dry-run]   OVMF_VARS: {}", args.ovmf_vars_path);
        if let Ok(block_image) = std::env::var("BLOCK_IMAGE") {
            if !block_image.is_empty() {
                eprintln!("[dry-run]   BLOCK_IMAGE: {}", block_image);
            }
        }
        return Ok(());
    }

    let acceleration = select_acceleration("qemu-system-x86_64", &args.arch, args.accel)?;
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

    run_command(
        make_x86_64_command(args, &ovmf_vars_runtime, &acceleration),
        args.verbose,
        "qemu-system-x86_64",
    )
}

pub fn run_qemu_aarch64(args: &RunQemuArgs) -> Result<()> {
    if args.arch != Arch::Aarch64 {
        bail!("run_qemu_aarch64 called with non-aarch64 arch");
    }
    if args.platform != Platform::Qemu {
        bail!("run_qemu_aarch64 called with non-qemu platform");
    }
    validate_smp(args.smp)?;

    if args.dry_run {
        eprintln!("[dry-run] qemu-system-aarch64 ...");
        eprintln!("[dry-run]   smp: {}", args.smp);
        eprintln!(
            "[dry-run]   hardware acceleration: {}",
            acceleration_policy_name(args.accel)
        );
        eprintln!("[dry-run]   U-Boot: {}", args.uboot_binary_path);
        eprintln!("[dry-run]   img: {}", args.img_path);
        return Ok(());
    }
    if !args.uboot_binary_path.is_file() {
        bail!(
            "AArch64 U-Boot artifact does not exist: {}",
            args.uboot_binary_path
        );
    }
    let acceleration = select_acceleration("qemu-system-aarch64", &args.arch, args.accel)?;
    run_command(
        make_aarch64_command(args, &acceleration),
        args.verbose,
        "qemu-system-aarch64 (U-Boot)",
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SelectedAcceleration {
    Hardware(&'static str),
    Tcg,
}

fn validate_smp(smp: u16) -> Result<()> {
    if smp == 0 {
        bail!("QEMU SMP count must be greater than zero");
    }
    Ok(())
}

fn acceleration_policy_name(policy: HardwareAcceleration) -> &'static str {
    match policy {
        HardwareAcceleration::Auto => "auto",
        HardwareAcceleration::On => "on",
        HardwareAcceleration::Off => "off",
    }
}

fn select_acceleration(
    qemu_binary: &str,
    guest_arch: &Arch,
    policy: HardwareAcceleration,
) -> Result<SelectedAcceleration> {
    if policy == HardwareAcceleration::Off {
        return Ok(SelectedAcceleration::Tcg);
    }

    if !host_can_accelerate(guest_arch) {
        if policy == HardwareAcceleration::On {
            bail!(
                "hardware acceleration requires matching host and guest architectures \
                 (host={}, guest={:?})",
                std::env::consts::ARCH,
                guest_arch
            );
        }
        return Ok(SelectedAcceleration::Tcg);
    }

    let output = Command::new(qemu_binary)
        .arg("-accel")
        .arg("help")
        .output()
        .with_context(|| format!("query accelerators supported by {}", qemu_binary))?;
    if !output.status.success() {
        bail!("{} failed to report supported accelerators", qemu_binary);
    }

    let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
    help.push_str(&String::from_utf8_lossy(&output.stderr));
    if let Some(accelerator) = choose_hardware_accelerator(&help, std::env::consts::OS) {
        return Ok(SelectedAcceleration::Hardware(accelerator));
    }

    if policy == HardwareAcceleration::On {
        bail!(
            "{} has no usable hardware accelerator for this host; \
             use --accel off to select TCG",
            qemu_binary
        );
    }
    Ok(SelectedAcceleration::Tcg)
}

fn host_can_accelerate(guest_arch: &Arch) -> bool {
    matches!(
        (std::env::consts::ARCH, guest_arch),
        ("x86_64", Arch::X86_64) | ("aarch64", Arch::Aarch64)
    )
}

fn choose_hardware_accelerator(help: &str, host_os: &str) -> Option<&'static str> {
    let supported = |name: &str| help.lines().any(|line| line.trim() == name);
    let candidates: &[&str] = match host_os {
        "linux" => &["kvm"],
        "macos" => &["hvf"],
        "windows" => &["whpx"],
        "freebsd" | "netbsd" => &["nvmm"],
        _ => &[],
    };
    candidates
        .iter()
        .copied()
        .find(|candidate| supported(candidate))
}

fn configure_acceleration(
    command: &mut Command,
    acceleration: &SelectedAcceleration,
    emulated_cpu: &str,
) {
    match acceleration {
        SelectedAcceleration::Hardware(name) => {
            command.arg("-accel").arg(name);
            command.arg("-cpu").arg("host");
        }
        SelectedAcceleration::Tcg => {
            command.arg("-accel").arg("tcg");
            command.arg("-cpu").arg(emulated_cpu);
        }
    }
}

fn make_x86_64_command(
    args: &RunQemuArgs,
    ovmf_vars_runtime: &Utf8Path,
    acceleration: &SelectedAcceleration,
) -> Command {
    let mut command = Command::new("qemu-system-x86_64");
    configure_acceleration(&mut command, acceleration, "max");
    command.arg("-m").arg("4G");
    command.arg("-net").arg("none");
    command.arg("-serial").arg("mon:stdio");
    command.arg("-smp").arg(args.smp.to_string());

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

    if let Ok(block_image) = std::env::var("BLOCK_IMAGE") {
        if !block_image.is_empty() {
            let block_format =
                std::env::var("BLOCK_IMAGE_FORMAT").unwrap_or_else(|_| "raw".to_string());
            command.arg("-drive").arg(format!(
                "if=none,id=blk0,format={},file={}",
                block_format, block_image
            ));
            command
                .arg("-device")
                .arg("virtio-blk-pci,drive=blk0,disable-legacy=off,disable-modern=on");
        }
    }

    command
        .arg("-netdev")
        .arg("user,id=net0,hostfwd=tcp:127.0.0.1:1234-:80");
    command.arg("-device").arg("e1000,netdev=net0");
    configure_debugging(&mut command, args);
    command
}

fn make_aarch64_command(args: &RunQemuArgs, acceleration: &SelectedAcceleration) -> Command {
    let mut command = Command::new("qemu-system-aarch64");
    configure_acceleration(&mut command, acceleration, "cortex-a72");
    command.arg("-machine").arg("virt,gic-version=2");
    command.arg("-m").arg("4G");
    command.arg("-smp").arg(args.smp.to_string());
    command.arg("-net").arg("none");
    command.arg("-nographic");
    command.arg("-bios").arg(args.uboot_binary_path);
    command
        .arg("-drive")
        .arg(format!("if=none,id=boot,format=raw,file={}", args.img_path));
    command.arg("-device").arg("virtio-blk-pci,drive=boot");
    configure_debugging(&mut command, args);
    command
}

fn configure_debugging(command: &mut Command, args: &RunQemuArgs) {
    if args.enable_gdb {
        command.arg("-s");
    }
    if args.stop_at_start {
        command.arg("-S");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_the_native_accelerator_for_each_host() {
        let help = "Accelerators supported in QEMU binary:\nhvf\nkvm\nwhpx\nnvmm\ntcg\n";
        assert_eq!(choose_hardware_accelerator(help, "macos"), Some("hvf"));
        assert_eq!(choose_hardware_accelerator(help, "linux"), Some("kvm"));
        assert_eq!(choose_hardware_accelerator(help, "windows"), Some("whpx"));
        assert_eq!(choose_hardware_accelerator(help, "freebsd"), Some("nvmm"));
    }

    #[test]
    fn does_not_treat_tcg_as_hardware_acceleration() {
        let help = "Accelerators supported in QEMU binary:\ntcg\n";
        assert_eq!(choose_hardware_accelerator(help, "linux"), None);
        assert_eq!(choose_hardware_accelerator(help, "macos"), None);
    }
}
