use camino::Utf8PathBuf;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
}

#[derive(Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum Platform {
    Qemu,
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
}
