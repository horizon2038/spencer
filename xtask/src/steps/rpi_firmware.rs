use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

const RPI_FIRMWARE_REPOSITORY: &str = "https://github.com/raspberrypi/firmware.git";
const RPI_FIRMWARE_VERSION: &str = "1.20260521";

const REQUIRED_BOOT_FILES: &[&str] = &[
    "bootcode.bin",
    "start4.elf",
    "fixup4.dat",
    "bcm2711-rpi-4-b.dtb",
    "LICENCE.broadcom",
    "COPYING.linux",
    "overlays/disable-bt.dtbo",
];

#[derive(Clone, Debug)]
pub struct BuildRpiFirmwareArgs<'a> {
    pub out_base: &'a Utf8Path,
    pub verbose: bool,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct RpiFirmwareArtifacts {
    pub boot_dir: Utf8PathBuf,
}

pub fn build_rpi_firmware(
    repo_root: &Utf8Path,
    args: &BuildRpiFirmwareArgs,
) -> Result<RpiFirmwareArtifacts> {
    let source_override = environment_path(repo_root, "RPI_FIRMWARE_DIR")?;
    let tools_dir = repo_root.join("tools").join("raspberrypi-firmware");
    let cached_source = repo_root
        .join("build")
        .join("raspberrypi-firmware")
        .join(RPI_FIRMWARE_VERSION)
        .join("source");

    let source_root = source_override.clone().unwrap_or_else(|| {
        if firmware_boot_dir(&tools_dir).is_some() {
            tools_dir
        } else {
            cached_source
        }
    });
    let artifact_boot_dir = args.out_base.join("raspberrypi-firmware").join("boot");

    if args.dry_run {
        if firmware_boot_dir(&source_root).is_none() {
            eprintln!(
                "[dry-run] git clone --depth 1 --branch {} --filter=blob:none --sparse {} {}",
                RPI_FIRMWARE_VERSION, RPI_FIRMWARE_REPOSITORY, source_root
            );
            eprintln!("[dry-run] git sparse-checkout Raspberry Pi 4 boot firmware");
        }
        eprintln!("[dry-run] install Raspberry Pi firmware:");
        eprintln!("[dry-run]   source: {}", source_root);
        eprintln!("[dry-run]   output: {}", artifact_boot_dir);
        return Ok(RpiFirmwareArtifacts {
            boot_dir: artifact_boot_dir,
        });
    }

    if firmware_boot_dir(&source_root).is_none() {
        if source_override.is_some() {
            bail!(
                "RPI_FIRMWARE_DIR does not contain Raspberry Pi 4 boot firmware: {}",
                source_root
            );
        }
        if source_root.exists() {
            bail!(
                "cached Raspberry Pi firmware checkout is incomplete: {}; remove that cache or set RPI_FIRMWARE_DIR",
                source_root
            );
        }
        clone_rpi_firmware(&source_root, args.verbose)?;
    }

    let source_boot_dir = firmware_boot_dir(&source_root).with_context(|| {
        format!(
            "Raspberry Pi firmware checkout is incomplete: {}",
            source_root
        )
    })?;
    install_boot_files(&source_boot_dir, &artifact_boot_dir)?;

    if args.verbose {
        eprintln!("[raspberrypi-firmware] installed: {}", artifact_boot_dir);
    }
    Ok(RpiFirmwareArtifacts {
        boot_dir: artifact_boot_dir,
    })
}

fn clone_rpi_firmware(destination: &Utf8Path, verbose: bool) -> Result<()> {
    let parent = destination
        .parent()
        .context("Raspberry Pi firmware source path has no parent")?;
    std::fs::create_dir_all(parent.as_std_path())
        .with_context(|| format!("create Raspberry Pi firmware cache parent: {}", parent))?;

    let mut clone = Command::new("git");
    clone
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(RPI_FIRMWARE_VERSION)
        .arg("--filter=blob:none")
        .arg("--sparse")
        .arg(RPI_FIRMWARE_REPOSITORY)
        .arg(destination.as_std_path());
    run_command(clone, verbose, "clone Raspberry Pi firmware")?;

    let mut sparse_checkout = Command::new("git");
    sparse_checkout
        .arg("-C")
        .arg(destination.as_std_path())
        .arg("sparse-checkout")
        .arg("set")
        .arg("--no-cone");
    for file in REQUIRED_BOOT_FILES {
        sparse_checkout.arg(format!("boot/{}", file));
    }
    run_command(
        sparse_checkout,
        verbose,
        "select Raspberry Pi 4 boot firmware",
    )
}

fn install_boot_files(source_boot_dir: &Utf8Path, destination: &Utf8Path) -> Result<()> {
    for relative_path in REQUIRED_BOOT_FILES {
        let source = source_boot_dir.join(relative_path);
        if !source.is_file() {
            bail!("required Raspberry Pi firmware file is missing: {}", source);
        }
        let target = destination.join(relative_path);
        let parent = target
            .parent()
            .with_context(|| format!("firmware artifact has no parent: {}", target))?;
        std::fs::create_dir_all(parent.as_std_path()).with_context(|| {
            format!(
                "create Raspberry Pi firmware artifact directory: {}",
                parent
            )
        })?;
        std::fs::copy(source.as_std_path(), target.as_std_path())
            .with_context(|| format!("copy Raspberry Pi firmware: {} -> {}", source, target))?;
    }
    Ok(())
}

fn firmware_boot_dir(root: &Utf8Path) -> Option<Utf8PathBuf> {
    for candidate in [root.to_path_buf(), root.join("boot")] {
        if REQUIRED_BOOT_FILES
            .iter()
            .all(|relative_path| candidate.join(relative_path).is_file())
        {
            return Some(candidate);
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_repository_root_or_its_boot_directory() {
        let temporary =
            std::env::temp_dir().join(format!("spencer-rpi-firmware-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        let boot = temporary.join("boot");
        for file in REQUIRED_BOOT_FILES {
            let path = boot.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).expect("create parent");
            std::fs::write(path, b"firmware").expect("write firmware fixture");
        }

        let root = Utf8PathBuf::from_path_buf(temporary.clone()).expect("UTF-8 temporary path");
        assert_eq!(firmware_boot_dir(&root), Some(root.join("boot")));
        assert_eq!(
            firmware_boot_dir(&root.join("boot")),
            Some(root.join("boot"))
        );

        std::fs::remove_dir_all(temporary).expect("remove temporary firmware fixture");
    }
}
