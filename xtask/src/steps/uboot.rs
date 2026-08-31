use crate::cli::{Arch, Platform};
use crate::steps::process::run_command;
use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use std::process::Command;

const UBOOT_REPOSITORY: &str = "https://source.denx.de/u-boot/u-boot.git";
const UBOOT_VERSION: &str = "v2025.10";
const UBOOT_DOCKERFILE: &str = "xtask/docker/u-boot.Dockerfile";
const UBOOT_DOCKER_IMAGE: &str = "spencer-u-boot-builder:v2025.10";

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
    if args.arch != Arch::Aarch64 {
        bail!("U-Boot build requires aarch64, got {:?}", args.arch);
    }
    let defconfig = uboot_defconfig(&args.platform)?;
    let platform_name = platform_name(&args.platform);

    let artifact_dir = args.out_base.join("u-boot");
    let artifact_binary = artifact_dir.join("u-boot.bin");

    let platform_binary_variable = match args.platform {
        Platform::Qemu => "UBOOT_QEMU_BIN",
        Platform::Rpi4b => "UBOOT_RPI4B_BIN",
    };
    if let Some(binary) = environment_path(repo_root, platform_binary_variable)? {
        return install_prebuilt(binary, artifact_binary, args, platform_binary_variable);
    }
    if let Some(binary) = environment_path(repo_root, "UBOOT_BIN")? {
        return install_prebuilt(binary, artifact_binary, args, "UBOOT_BIN");
    }

    let source_override = environment_path(repo_root, "UBOOT_SOURCE")?;
    let tools_dir = repo_root.join("tools").join("u-boot");
    if source_override.is_none() && !is_uboot_source(&tools_dir) {
        let (tools_binary, tools_binary_name) = match args.platform {
            Platform::Qemu => (tools_dir.join("u-boot.bin"), "tools/u-boot/u-boot.bin"),
            Platform::Rpi4b => (
                tools_dir.join("rpi4b").join("u-boot.bin"),
                "tools/u-boot/rpi4b/u-boot.bin",
            ),
        };
        if tools_binary.is_file() {
            return install_prebuilt(tools_binary, artifact_binary, args, tools_binary_name);
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
    let build_dir = args.out_base.join("u-boot").join("docker-build");
    let docker = environment_value("UBOOT_DOCKER", "docker")?;
    let docker_image_override = std::env::var("UBOOT_DOCKER_IMAGE").ok();
    let docker_image = docker_image_override
        .as_deref()
        .unwrap_or(UBOOT_DOCKER_IMAGE);

    if args.dry_run {
        if !is_uboot_source(&source_dir) {
            eprintln!(
                "[dry-run] git clone --depth 1 --branch {} {} {}",
                UBOOT_VERSION, UBOOT_REPOSITORY, source_dir
            );
        }
        if docker_image_override.is_none() {
            eprintln!(
                "[dry-run] {} build --file {} --tag {} {}",
                docker,
                repo_root.join(UBOOT_DOCKERFILE),
                docker_image,
                repo_root.join("xtask").join("docker")
            );
        }
        eprintln!(
            "[dry-run] {} run {} make {} (U-Boot)",
            docker, docker_image, defconfig
        );
        eprintln!("[dry-run]   /src   <- {} (read-only)", source_dir);
        eprintln!("[dry-run]   /build <- {}", build_dir);
        if args.platform == Platform::Rpi4b {
            eprintln!(
                "[dry-run] configure the Raspberry Pi PL011 early debug and serial-only console"
            );
        }
        eprintln!(
            "[dry-run] {} run {} make -j{} (U-Boot)",
            docker,
            docker_image,
            parallel_jobs()
        );
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

    if !executable_exists(&docker) {
        bail!(
            "Docker is required to build U-Boot from source; install Docker, set \
             UBOOT_DOCKER, or provide a prebuilt U-Boot with {} or UBOOT_BIN",
            platform_binary_variable
        );
    }
    if docker_image_override.is_none() {
        build_docker_image(repo_root, &docker, docker_image, args.verbose)?;
    }

    let user = host_user()?;
    let configure = docker_make_command(
        &docker,
        docker_image,
        &source_dir,
        &build_dir,
        &user,
        [defconfig],
    );
    run_command(
        configure,
        args.verbose,
        &format!("configure U-Boot for AArch64 {} in Docker", platform_name),
    )?;

    if args.platform == Platform::Rpi4b {
        let serial_config = docker_rpi4b_serial_config_command(
            &docker,
            docker_image,
            &source_dir,
            &build_dir,
            &user,
        );
        run_command(
            serial_config,
            args.verbose,
            "configure U-Boot PL011 early debug console for Raspberry Pi 4",
        )?;

        let olddefconfig = docker_make_command(
            &docker,
            docker_image,
            &source_dir,
            &build_dir,
            &user,
            ["olddefconfig"],
        );
        run_command(
            olddefconfig,
            args.verbose,
            "resolve U-Boot Raspberry Pi 4 serial configuration",
        )?;
    }

    let jobs = format!("-j{}", parallel_jobs());
    let mut build_args = vec![jobs.as_str()];
    if args.verbose {
        build_args.push("V=1");
    }
    let build = docker_make_command(
        &docker,
        docker_image,
        &source_dir,
        &build_dir,
        &user,
        build_args,
    );
    run_command(
        build,
        args.verbose,
        &format!("build U-Boot for AArch64 {} in Docker", platform_name),
    )?;

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

fn uboot_defconfig(platform: &Platform) -> Result<&'static str> {
    match platform {
        Platform::Qemu => Ok("qemu_arm64_defconfig"),
        Platform::Rpi4b => Ok("rpi_4_defconfig"),
    }
}

fn platform_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Qemu => "qemu",
        Platform::Rpi4b => "rpi4b",
    }
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

fn environment_value(name: &str, default: &str) -> Result<String> {
    let value = std::env::var(name).unwrap_or_else(|_| default.to_string());
    if value.is_empty() {
        bail!("{} must not be empty", name);
    }
    Ok(value)
}

fn build_docker_image(
    repo_root: &Utf8Path,
    docker: &str,
    image: &str,
    verbose: bool,
) -> Result<()> {
    let dockerfile = repo_root.join(UBOOT_DOCKERFILE);
    if !dockerfile.is_file() {
        bail!("U-Boot Dockerfile does not exist: {}", dockerfile);
    }
    let context = dockerfile
        .parent()
        .context("U-Boot Dockerfile path has no parent")?;
    let mut build = Command::new(docker);
    build
        .arg("build")
        .arg("--file")
        .arg(dockerfile.as_std_path())
        .arg("--tag")
        .arg(image)
        .arg(context.as_std_path());
    run_command(build, verbose, "build U-Boot Docker image")
}

fn host_user() -> Result<String> {
    let uid = numeric_host_id("-u")?;
    let gid = numeric_host_id("-g")?;
    Ok(format!("{}:{}", uid, gid))
}

fn numeric_host_id(argument: &str) -> Result<String> {
    let output = Command::new("id")
        .arg(argument)
        .output()
        .with_context(|| format!("run id {} for the U-Boot Docker container", argument))?;
    if !output.status.success() {
        bail!("id {} failed with {}", argument, output.status);
    }
    let value = std::str::from_utf8(&output.stdout)
        .context("id output is not valid UTF-8")?
        .trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!(
            "id {} returned an invalid numeric ID: {:?}",
            argument,
            value
        );
    }
    Ok(value.to_string())
}

fn docker_make_command<'a>(
    docker: &str,
    image: &str,
    source_dir: &Utf8Path,
    build_dir: &Utf8Path,
    user: &str,
    make_arguments: impl IntoIterator<Item = &'a str>,
) -> Command {
    let mut command = Command::new(docker);
    command
        .arg("run")
        .arg("--rm")
        .arg("--user")
        .arg(user)
        .arg("--env")
        .arg("HOME=/tmp")
        .arg("--volume")
        .arg(format!("{}:/src:ro", source_dir))
        .arg("--volume")
        .arg(format!("{}:/build", build_dir))
        .arg(image)
        .arg("make")
        .arg("-C")
        .arg("/src")
        .arg("O=/build")
        .arg("CROSS_COMPILE=aarch64-linux-gnu-")
        .args(make_arguments);
    command
}

fn docker_rpi4b_serial_config_command(
    docker: &str,
    image: &str,
    source_dir: &Utf8Path,
    build_dir: &Utf8Path,
    user: &str,
) -> Command {
    let mut command = docker_container_command(docker, image, source_dir, build_dir, user);
    command
        .arg("/src/scripts/config")
        .arg("--file")
        .arg("/build/.config")
        .arg("--enable")
        .arg("REQUIRE_SERIAL_CONSOLE")
        .arg("--disable")
        .arg("VIDEO")
        .arg("--disable")
        .arg("USB_KEYBOARD")
        .arg("--enable")
        .arg("DEBUG_UART")
        .arg("--enable")
        .arg("DEBUG_UART_PL011")
        .arg("--enable")
        .arg("DEBUG_UART_ANNOUNCE")
        .arg("--disable")
        .arg("DEBUG_UART_SKIP_INIT")
        .arg("--set-val")
        .arg("DEBUG_UART_BASE")
        .arg("0xfe201000")
        .arg("--set-val")
        .arg("DEBUG_UART_CLOCK")
        .arg("48000000");
    command
}

fn docker_container_command(
    docker: &str,
    image: &str,
    source_dir: &Utf8Path,
    build_dir: &Utf8Path,
    user: &str,
) -> Command {
    let mut command = Command::new(docker);
    command
        .arg("run")
        .arg("--rm")
        .arg("--user")
        .arg(user)
        .arg("--env")
        .arg("HOME=/tmp")
        .arg("--volume")
        .arg(format!("{}:/src:ro", source_dir))
        .arg("--volume")
        .arg(format!("{}:/build", build_dir))
        .arg(image);
    command
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

    #[test]
    fn selects_the_platform_defconfig() {
        assert_eq!(
            uboot_defconfig(&Platform::Qemu).unwrap(),
            "qemu_arm64_defconfig"
        );
        assert_eq!(
            uboot_defconfig(&Platform::Rpi4b).unwrap(),
            "rpi_4_defconfig"
        );
    }

    #[test]
    fn builds_u_boot_with_the_container_cross_compiler() {
        let command = docker_make_command(
            "docker",
            "spencer-u-boot-builder:test",
            Utf8Path::new("/source"),
            Utf8Path::new("/output"),
            "501:20",
            ["rpi_4_defconfig"],
        );
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            arguments
                .iter()
                .any(|argument| argument == "/source:/src:ro")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "/output:/build")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "CROSS_COMPILE=aarch64-linux-gnu-")
        );
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "rpi_4_defconfig")
        );
    }

    #[test]
    fn configures_rpi4b_for_an_early_pl011_console() {
        let command = docker_rpi4b_serial_config_command(
            "docker",
            "spencer-u-boot-builder:test",
            Utf8Path::new("/source"),
            Utf8Path::new("/output"),
            "501:20",
        );
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        for required in [
            "REQUIRE_SERIAL_CONSOLE",
            "DEBUG_UART",
            "DEBUG_UART_PL011",
            "DEBUG_UART_ANNOUNCE",
            "0xfe201000",
            "48000000",
        ] {
            assert!(arguments.iter().any(|argument| argument == required));
        }
    }
}
