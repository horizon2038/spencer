use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};
use std::num::NonZeroU16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Platform {
    Qemu,
    Rpi4b,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum HardwareAcceleration {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug, Parser)]
#[command(author, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    Build(BuildArgs),
    Run(RunArgs),
}

#[derive(Clone, Debug, Parser)]
pub struct CommonArgs {
    #[arg(long, value_enum)]
    pub arch: Arch,

    #[arg(long, value_enum)]
    pub platform: Platform,

    #[arg(long)]
    pub release: bool,

    #[arg(long, default_value_t = false)]
    pub verbose: bool,

    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Cargo manifest for the OS payload. Defaults to ./core/Cargo.toml.
    #[arg(long)]
    pub os_manifest: Option<Utf8PathBuf>,

    /// Custom target JSON for the OS payload. Defaults to ./Nun/arch/<arch>-unknown-a9n.json.
    #[arg(long)]
    pub os_target_json: Option<Utf8PathBuf>,

    /// Output binary name produced by the OS payload package. Defaults to core.
    #[arg(long, default_value = "core")]
    pub os_binary: String,
}

#[derive(Clone, Debug, Parser)]
pub struct BuildArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Clone, Debug, Parser)]
pub struct RunArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    #[arg(long, default_value_t = false)]
    pub gdb: bool,

    #[arg(long, default_value_t = false)]
    pub stop: bool,

    /// Build A9N with A9N_CONFIG_ENABLE_SMP=ON.
    #[arg(long, default_value_t = false)]
    pub enable_smp: bool,

    /// Number of virtual CPUs exposed by QEMU.
    #[arg(long, default_value = "1")]
    pub smp: NonZeroU16,

    /// QEMU hardware acceleration policy.
    #[arg(long, value_enum, default_value_t = HardwareAcceleration::Auto)]
    pub accel: HardwareAcceleration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_defaults_to_one_cpu_and_auto_acceleration() {
        let cli = Cli::try_parse_from(["xtask", "run", "--arch", "aarch64", "--platform", "qemu"])
            .expect("parse run arguments");

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert!(!args.enable_smp);
        assert_eq!(args.smp.get(), 1);
        assert_eq!(args.accel, HardwareAcceleration::Auto);
    }

    #[test]
    fn run_accepts_explicit_smp_and_acceleration_policy() {
        let cli = Cli::try_parse_from([
            "xtask",
            "run",
            "--arch",
            "x86-64",
            "--platform",
            "qemu",
            "--enable-smp",
            "--smp",
            "8",
            "--accel",
            "off",
        ])
        .expect("parse run arguments");

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert!(args.enable_smp);
        assert_eq!(args.smp.get(), 8);
        assert_eq!(args.accel, HardwareAcceleration::Off);
    }

    #[test]
    fn run_rejects_zero_cpus() {
        let result = Cli::try_parse_from([
            "xtask",
            "run",
            "--arch",
            "x86-64",
            "--platform",
            "qemu",
            "--smp",
            "0",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn build_accepts_the_raspberry_pi_4_platform() {
        let cli = Cli::try_parse_from([
            "xtask",
            "build",
            "--arch",
            "aarch64",
            "--platform",
            "rpi4b",
            "--release",
        ])
        .expect("parse Raspberry Pi 4 build arguments");

        let Command::Build(args) = cli.command else {
            panic!("expected build command");
        };
        assert_eq!(args.common.arch, Arch::Aarch64);
        assert_eq!(args.common.platform, Platform::Rpi4b);
        assert!(args.common.release);
    }
}
