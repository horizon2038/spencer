use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

const UBOOT_REPOSITORY: &str = "https://source.denx.de/u-boot/u-boot.git";
const UBOOT_VERSION: &str = "v2025.10";
const UBOOT_DEFCONFIG: &str = "qemu_arm64_defconfig";

#[derive(Clone, Debug)]
pub struct BuildUbootArgs<'a> {
    pub arch: Arch,
    pub platform: Platform,
    pub out_base: &'a Utf8Path,
    pub verbose: bool,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct UbootArtifacts {
    pub binary_path: Utf8PathBuf,
}

pub fn build_uboot(repo_root: &Utf8Path, args: &BuildUbootArgs) -> Result<UbootArtifacts> {
    if args.arch != Arch::Aarch64 || args.platform != Platform::Qemu {
        bail!("U-Boot build is currently supported only for aarch64/qemu");
    }

    let artifact_dir = args.out_base.join("u-boot");
    let artifact_binary = artifact_dir.join("u-boot.bin");

    if let Some(binary) = environment_path(repo_root, "UBOOT_BIN")? {
        return install_prebuilt(binary, artifact_binary, args, "UBOOT_BIN");
    }

    let source_override = environment_path(repo_root, "UBOOT_SOURCE")?;
    let tools_dir = repo_root.join("tools").join("u-boot");
    if source_override.is_none() && !is_uboot_source(&tools_dir) {
        let tools_binary = tools_dir.join("u-boot.bin");
        if tools_binary.is_file() {
            return install_prebuilt(
                tools_binary,
                artifact_binary,
                args,
                "tools/u-boot/u-boot.bin",
            );
        }
    }

    let source_dir = source_override.clone().unwrap_or_else(|| {
        if is_uboot_source(&tools_dir) {
            tools_dir
        } else {
            repo_root
                .join("build")
                .join("u-boot")
                .join(UBOOT_VERSION)
                .join("source")
        }
    });
    let build_dir = args.out_base.join("u-boot").join("build");

    if args.dry_run {
        if !is_uboot_source(&source_dir) {
            eprintln!(
                "[dry-run] git clone --depth 1 --branch {} {} {}",
                UBOOT_VERSION, UBOOT_REPOSITORY, source_dir
            );
        }
        eprintln!("[dry-run] make {} (U-Boot)", UBOOT_DEFCONFIG);
        eprintln!("[dry-run]   source: {}", source_dir);
        eprintln!("[dry-run]   O={}", build_dir);
        eprintln!("[dry-run] make -j{} (U-Boot)", parallel_jobs());
        eprintln!("[dry-run]   output: {}", artifact_binary);
        return Ok(UbootArtifacts {
            binary_path: artifact_binary,
        });
    }

    if !is_uboot_source(&source_dir) {
        if source_override.is_some() {
            bail!(
                "UBOOT_SOURCE is not a U-Boot source tree (Makefile not found): {}",
                source_dir
            );
        }
        clone_uboot(&source_dir, args.verbose)?;
    }

    if !is_uboot_source(&source_dir) {
        bail!("U-Boot source tree is incomplete: {}", source_dir);
    }

    std::fs::create_dir_all(build_dir.as_std_path())
        .with_context(|| format!("create U-Boot build directory: {}", build_dir))?;
    std::fs::create_dir_all(artifact_dir.as_std_path())
        .with_context(|| format!("create U-Boot artifact directory: {}", artifact_dir))?;

    let make = std::env::var("UBOOT_MAKE").unwrap_or_else(|_| "make".to_string());
    let cross_compile = cross_compile_prefix()?;

    let mut configure = Command::new(&make);
    configure
        .current_dir(source_dir.as_std_path())
        .arg(format!("O={}", build_dir))
        .arg(UBOOT_DEFCONFIG);
    apply_cross_compile(&mut configure, cross_compile.as_deref());
    run_command(configure, args.verbose, "configure U-Boot for QEMU AArch64")?;

    let mut build = Command::new(&make);
    build
        .current_dir(source_dir.as_std_path())
        .arg(format!("O={}", build_dir))
        .arg(format!("-j{}", parallel_jobs()));
    if args.verbose {
        build.arg("V=1");
    }
    apply_cross_compile(&mut build, cross_compile.as_deref());
    run_command(build, args.verbose, "build U-Boot for QEMU AArch64")?;

    let built_binary = build_dir.join("u-boot.bin");
    if !built_binary.is_file() {
        bail!("U-Boot build did not produce {}", built_binary);
    }
    copy_binary(&built_binary, &artifact_binary)?;

    if args.verbose {
        eprintln!("[u-boot] installed: {}", artifact_binary);
    }
    Ok(UbootArtifacts {
        binary_path: artifact_binary,
    })
}

fn install_prebuilt(
    source: Utf8PathBuf,
    destination: Utf8PathBuf,
    args: &BuildUbootArgs,
    source_name: &str,
) -> Result<UbootArtifacts> {
    if args.dry_run {
        eprintln!(
            "[dry-run] U-Boot binary ({}) {} -> {}",
            source_name, source, destination
        );
        return Ok(UbootArtifacts {
            binary_path: destination,
        });
    }
    if !source.is_file() {
        bail!("U-Boot binary does not exist: {}", source);
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent.as_std_path())
            .with_context(|| format!("create U-Boot artifact directory: {}", parent))?;
    }
    copy_binary(&source, &destination)?;
    if args.verbose {
        eprintln!("[u-boot] installed {}: {}", source_name, destination);
    }
    Ok(UbootArtifacts {
        binary_path: destination,
    })
}

fn clone_uboot(source_dir: &Utf8Path, verbose: bool) -> Result<()> {
    let parent = source_dir
        .parent()
        .context("U-Boot source path has no parent")?;
    std::fs::create_dir_all(parent.as_std_path())
        .with_context(|| format!("create U-Boot source parent: {}", parent))?;

    let mut clone = Command::new("git");
    clone
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(UBOOT_VERSION)
        .arg(UBOOT_REPOSITORY)
        .arg(source_dir.as_std_path());
    run_command(clone, verbose, "clone U-Boot source")
}

fn copy_binary(source: &Utf8Path, destination: &Utf8Path) -> Result<()> {
    if source == destination {
        return Ok(());
    }
    std::fs::copy(source.as_std_path(), destination.as_std_path())
        .with_context(|| format!("copy U-Boot binary: {} -> {}", source, destination))?;
    Ok(())
}

fn environment_path(repo_root: &Utf8Path, name: &str) -> Result<Option<Utf8PathBuf>> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let path = Utf8PathBuf::from_path_buf(value.into())
        .map_err(|_| anyhow::anyhow!("{} is not valid UTF-8", name))?;
    Ok(Some(if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    }))
}

fn is_uboot_source(path: &Utf8Path) -> bool {
    path.join("Makefile").is_file() && path.join("configs").is_dir()
}

fn apply_cross_compile(command: &mut Command, prefix: Option<&str>) {
    if let Some(prefix) = prefix {
        command.env("CROSS_COMPILE", prefix);
    }
}

fn cross_compile_prefix() -> Result<Option<String>> {
    if let Ok(prefix) = std::env::var("UBOOT_CROSS_COMPILE") {
        return Ok(Some(prefix));
    }
    if let Ok(prefix) = std::env::var("CROSS_COMPILE") {
        if !prefix.is_empty() {
            return Ok(Some(prefix));
        }
    }

    for prefix in ["aarch64-linux-gnu-", "aarch64-none-elf-"] {
        if executable_exists(&format!("{}gcc", prefix)) {
            return Ok(Some(prefix.to_string()));
        }
    }

    if std::env::consts::OS == "linux" && std::env::consts::ARCH == "aarch64" {
        return Ok(None);
    }

    bail!(
        "an AArch64 U-Boot cross compiler is required; set \
         UBOOT_CROSS_COMPILE (for example, aarch64-linux-gnu-), \
         or set UBOOT_BIN to a prebuilt QEMU virt U-Boot binary"
    )
}

fn executable_exists(command: &str) -> bool {
    let command_path = std::path::Path::new(command);
    if command_path.components().count() > 1 {
        return command_path.is_file();
    }
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(command))
                .any(|candidate| candidate.is_file())
        })
        .unwrap_or(false)
}

fn parallel_jobs() -> usize {
    std::env::var("UBOOT_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .or_else(|| std::thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_uboot_source_tree() {
        let temporary =
            std::env::temp_dir().join(format!("spencer-uboot-source-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        std::fs::create_dir_all(temporary.join("configs")).expect("create configs");
        std::fs::write(temporary.join("Makefile"), b"all:\n").expect("write Makefile");
        let temporary = Utf8PathBuf::from_path_buf(temporary).expect("UTF-8 temp path");

        assert!(is_uboot_source(&temporary));
        std::fs::remove_dir_all(temporary).expect("remove temporary source");
    }

    #[test]
    fn rejects_a_directory_containing_only_a_binary() {
        let temporary =
            std::env::temp_dir().join(format!("spencer-uboot-binary-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        std::fs::create_dir_all(&temporary).expect("create temporary directory");
        std::fs::write(temporary.join("u-boot.bin"), b"u-boot").expect("write binary");
        let temporary = Utf8PathBuf::from_path_buf(temporary).expect("UTF-8 temp path");

        assert!(!is_uboot_source(&temporary));
        std::fs::remove_dir_all(temporary).expect("remove temporary directory");
    }
}
