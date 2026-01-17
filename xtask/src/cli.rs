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
