use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct BuildNunOsArgs {
    pub arch: Arch,
    pub platform: Platform,
    pub release: bool,
    pub verbose: bool,
    pub dry_run: bool,

    pub use_nightly_build_std: bool,
    pub os_manifest: Option<Utf8PathBuf>,
    pub os_target_json: Option<Utf8PathBuf>,
    pub os_binary: String,
}

#[derive(Clone, Debug)]
pub struct NunOsArtifacts {
    pub cargo_target_json: Utf8PathBuf,
    pub cargo_target_dir: Utf8PathBuf,
    pub executable_path: Utf8PathBuf,
}

pub fn build_nun_os(repo_root: &Utf8Path, args: &BuildNunOsArgs) -> Result<NunOsArtifacts> {
    validate_supported(&args.arch, &args.platform)?;

    let os_manifest = args
        .os_manifest
        .as_ref()
        .map(|path| resolve_path(repo_root, path))
        .unwrap_or_else(|| repo_root.join("core").join("Cargo.toml"));

    let os_dir = os_manifest
        .parent()
        .with_context(|| format!("OS manifest has no parent: {}", os_manifest))?
        .to_path_buf();

    let target_json = args
        .os_target_json
        .as_ref()
        .map(|path| resolve_path(repo_root, path))
        .unwrap_or_else(|| nun_custom_target_json(repo_root, &args.arch));

    let out_base = repo_root.join("out").join(format!(
        "{}-{}-{}",
        to_arch_name(&args.arch),
        to_platform_name(&args.platform),
        if args.release { "release" } else { "debug" }
    ));

    let cargo_target_dir = out_base.join("nun_os_target_dir");
    let target_dir_name = target_json
        .file_stem()
        .with_context(|| format!("target json has no file stem: {}", target_json))?;
    let profile_dir = if args.release { "release" } else { "debug" };
    let executable_path = cargo_target_dir
        .join(target_dir_name)
        .join(profile_dir)
        .join(&args.os_binary);

    if args.dry_run {
        eprintln!("[dry-run] cargo build (Nun OS)");
        eprintln!("[dry-run]   dir: {}", os_dir);
        eprintln!("[dry-run]   --manifest-path {}", os_manifest);
        eprintln!("[dry-run]   --target {}", target_json);
        eprintln!("[dry-run]   CARGO_TARGET_DIR={}", cargo_target_dir);
        if args.release {
            eprintln!("[dry-run]   --release");
        }
        if args.use_nightly_build_std {
            eprintln!(
                "[dry-run]   -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem"
            );
            eprintln!("[dry-run]   -Z json-target-spec");
        }

        return Ok(NunOsArtifacts {
            cargo_target_json: target_json,
            cargo_target_dir,
            executable_path,
        });
    }

    std::fs::create_dir_all(&cargo_target_dir)
        .with_context(|| format!("create cargo target dir: {}", cargo_target_dir))?;

    if !os_manifest.exists() {
        bail!("OS manifest not found: {}", os_manifest);
    }

    if !target_json.exists() {
        bail!("Nun custom target json not found: {}", target_json);
    }

    let mut command = Command::new("cargo");
    command.current_dir(repo_root);

    command.arg("build");
    command.arg("--manifest-path");
    command.arg(&os_manifest);
    command.arg("--target");
    command.arg(&target_json);

    if args.release {
        command.arg("--release");
    }

    command.arg("-Z");
    command.arg("build-std=core,alloc,compiler_builtins");

    command.arg("-Z");
    command.arg("build-std-features=compiler-builtins-mem");

    command.arg("-Z");
    command.arg("json-target-spec");

    command.env("CARGO_TARGET_DIR", cargo_target_dir.as_str());

    run_command(command, args.verbose, "cargo build (Nun OS)")?;

    Ok(NunOsArtifacts {
        cargo_target_json: target_json,
        cargo_target_dir,
        executable_path,
    })
}

fn resolve_path(repo_root: &Utf8Path, path: &Utf8Path) -> Utf8PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn nun_custom_target_json(repo_root: &Utf8Path, arch: &Arch) -> Utf8PathBuf {
    repo_root
        .join("Nun")
        .join("arch")
        .join(format!("{}-unknown-a9n.json", to_arch_name(arch)))
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
