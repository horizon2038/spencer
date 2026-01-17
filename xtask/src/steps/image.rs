use anyhow::{Context, Result};
use camino::Utf8Path;
use fscommon::BufStream;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

pub struct BuildImgArgs<'a> {
    pub img_path: &'a Utf8Path,

    pub bootx64_efi_source_path: &'a Utf8Path,
    pub init_elf_source_path: &'a Utf8Path,
    pub kernel_elf_source_path: &'a Utf8Path,

    pub image_size_mib: u64,
    pub verbose: bool,
    pub dry_run: bool,
}

pub fn build_fat_img(args: &BuildImgArgs) -> Result<()> {
    if args.dry_run {
        eprintln!("[dry-run] create img: {}", args.img_path);
        eprintln!(
            "[dry-run]   /EFI/BOOT/BOOTX64.EFI <- {}",
            args.bootx64_efi_source_path
        );
        eprintln!(
            "[dry-run]   /kernel/init.elf      <- {}",
            args.init_elf_source_path
        );
        eprintln!(
            "[dry-run]   /kernel/kernel.elf    <- {}",
            args.kernel_elf_source_path
        );
        return Ok(());
    }

    let parent = args.img_path.parent().context("img_path has no parent")?;
    std::fs::create_dir_all(parent.as_std_path())
        .with_context(|| format!("create img parent dir: {}", parent))?;

    let image_size_bytes = args.image_size_mib * 1024 * 1024;

    // Create & size
    {
        let file = File::create(args.img_path.as_std_path())
            .with_context(|| format!("create img file: {}", args.img_path))?;
        file.set_len(image_size_bytes)
            .with_context(|| format!("set img size: {} bytes", image_size_bytes))?;
    }

    // Format FAT32
    {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(args.img_path.as_std_path())
            .with_context(|| format!("open img for format: {}", args.img_path))?;

        let stream = BufStream::new(file);

        let format_options = fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32);

        fatfs::format_volume(stream, format_options).context("format FAT volume")?;
    }

    // Open FS and write files
    {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(args.img_path.as_std_path())
            .with_context(|| format!("open img for fs: {}", args.img_path))?;

        let stream = BufStream::new(file);

        let fs = fatfs::FileSystem::new(stream, fatfs::FsOptions::new())
            .context("open FAT filesystem")?;

        {
            let root = fs.root_dir();

            let efi_dir = ensure_dir(&root, "EFI")?;
            let boot_dir = ensure_dir(&efi_dir, "BOOT")?;
            let kernel_dir = ensure_dir(&root, "kernel")?;

            write_file_from_host(&boot_dir, "BOOTX64.EFI", args.bootx64_efi_source_path)?;
            write_file_from_host(&kernel_dir, "init.elf", args.init_elf_source_path)?;
            write_file_from_host(&kernel_dir, "kernel.elf", args.kernel_elf_source_path)?;
        }

        fs.unmount().context("unmount FAT filesystem")?;
    }

    if args.verbose {
        eprintln!("[img] created: {}", args.img_path);
    }

    Ok(())
}

fn ensure_dir<'a>(
    parent: &'a fatfs::Dir<BufStream<File>>,
    name: &str,
) -> Result<fatfs::Dir<'a, BufStream<File>>> {
    if let Ok(dir) = parent.open_dir(name) {
        return Ok(dir);
    }

    parent
        .create_dir(name)
        .with_context(|| format!("create dir: {}", name))?;
    parent
        .open_dir(name)
        .with_context(|| format!("open dir: {}", name))
}

fn write_file_from_host(
    dir: &fatfs::Dir<BufStream<File>>,
    file_name: &str,
    host_path: &Utf8Path,
) -> Result<()> {
    let mut host_file = File::open(host_path.as_std_path())
        .with_context(|| format!("open host file: {}", host_path))?;

    if dir.open_file(file_name).is_ok() {
        dir.remove(file_name)
            .with_context(|| format!("remove existing file: {}", file_name))?;
    }

    let mut fat_file = dir
        .create_file(file_name)
        .with_context(|| format!("create fat file: {}", file_name))?;

    let mut buffer = [0u8; 1024 * 64];
    loop {
        let read_size = host_file.read(&mut buffer)?;
        if read_size == 0 {
            break;
        }

        fat_file.write_all(&buffer[..read_size])?;
    }

    fat_file.flush()?;
    Ok(())
}
