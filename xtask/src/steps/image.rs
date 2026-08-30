use anyhow::{Context, Result};
use camino::Utf8Path;
use fscommon::{BufStream, StreamSlice};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

pub struct BuildImgArgs<'a> {
    pub img_path: &'a Utf8Path,

    pub bootx64_efi_source_path: &'a Utf8Path,
    pub init_elf_source_path: &'a Utf8Path,
    pub kernel_elf_source_path: &'a Utf8Path,

    pub image_size_mib: u64,
    pub verbose: bool,
    pub dry_run: bool,
}

pub struct BuildUbootImgArgs<'a> {
    pub img_path: &'a Utf8Path,
    pub uboot_binary_source_path: &'a Utf8Path,
    pub init_elf_source_path: &'a Utf8Path,
    pub kernel_image_source_path: &'a Utf8Path,
    pub image_size_mib: u64,
    pub verbose: bool,
    pub dry_run: bool,
}

const AARCH64_UBOOT_COMMANDS: &[u8] = br#"echo Booting A9N through U-Boot...
if test -z "${kernel_addr_r}"; then setenv kernel_addr_r 0x40200000; fi
if test -z "${ramdisk_addr_r}"; then setenv ramdisk_addr_r 0x48000000; fi
if test -z "${fdt_addr_r}"; then setenv fdt_addr_r 0x49000000; fi
load ${devtype} ${devnum}:${distro_bootpart} ${kernel_addr_r} /kernel/kernel.img
setexpr a9n_image_size_addr ${kernel_addr_r} + 0x10
setexpr.l a9n_kernel_size *${a9n_image_size_addr}
setexpr a9n_safe_ramdisk ${kernel_addr_r} + ${a9n_kernel_size}
setexpr a9n_safe_ramdisk ${a9n_safe_ramdisk} + 0x200000
if itest ${ramdisk_addr_r} -lt ${a9n_safe_ramdisk}; then setenv ramdisk_addr_r ${a9n_safe_ramdisk}; fi
load ${devtype} ${devnum}:${distro_bootpart} ${ramdisk_addr_r} /kernel/init.elf
setenv ramdisk_size ${filesize}
if test -n "${fdt_addr}"; then setenv a9n_fdt_source ${fdt_addr}; else setenv a9n_fdt_source ${fdtcontroladdr}; fi
fdt addr ${a9n_fdt_source}
fdt header get a9n_fdt_size totalsize
setexpr a9n_fdt_capacity ${a9n_fdt_size} + 0x10000
fdt move ${a9n_fdt_source} ${fdt_addr_r} ${a9n_fdt_capacity}
booti ${kernel_addr_r} ${ramdisk_addr_r}:${ramdisk_size} ${fdt_addr_r}
"#;

const DISK_SECTOR_SIZE: u64 = 512;
const BOOT_PARTITION_START_SECTOR: u64 = 2048;

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

pub fn build_uboot_fat_img(args: &BuildUbootImgArgs) -> Result<()> {
    if args.dry_run {
        eprintln!("[dry-run] create U-Boot img: {}", args.img_path);
        eprintln!("[dry-run]   /boot.scr            <- generated U-Boot script image");
        eprintln!(
            "[dry-run]   /u-boot.bin          <- {}",
            args.uboot_binary_source_path
        );
        eprintln!(
            "[dry-run]   /kernel/init.elf      <- {}",
            args.init_elf_source_path
        );
        eprintln!(
            "[dry-run]   /kernel/kernel.img    <- {}",
            args.kernel_image_source_path
        );
        return Ok(());
    }

    let parent = args.img_path.parent().context("img_path has no parent")?;
    std::fs::create_dir_all(parent.as_std_path())
        .with_context(|| format!("create img parent dir: {}", parent))?;

    let image_size_bytes = args.image_size_mib * 1024 * 1024;
    let partition_start = BOOT_PARTITION_START_SECTOR * DISK_SECTOR_SIZE;
    if image_size_bytes <= partition_start {
        anyhow::bail!("U-Boot image must be larger than the 1 MiB partition offset");
    }
    {
        let mut file = File::create(args.img_path.as_std_path())
            .with_context(|| format!("create img file: {}", args.img_path))?;
        file.set_len(image_size_bytes)
            .with_context(|| format!("set img size: {} bytes", image_size_bytes))?;
        write_mbr(&mut file, image_size_bytes)?;
    }
    {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(args.img_path.as_std_path())
            .with_context(|| format!("open img for format: {}", args.img_path))?;
        let partition = StreamSlice::new(file, partition_start, image_size_bytes)
            .context("open boot partition for format")?;
        fatfs::format_volume(
            BufStream::new(partition),
            fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32),
        )
        .context("format FAT volume")?;
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(args.img_path.as_std_path())
        .with_context(|| format!("open img for fs: {}", args.img_path))?;
    let partition =
        StreamSlice::new(file, partition_start, image_size_bytes).context("open boot partition")?;
    let fs = fatfs::FileSystem::new(BufStream::new(partition), fatfs::FsOptions::new())
        .context("open FAT boot filesystem")?;
    {
        let root = fs.root_dir();
        let kernel_dir = ensure_dir(&root, "kernel")?;
        let boot_dir = ensure_dir(&root, "boot")?;
        let boot_script = make_uboot_script_image(AARCH64_UBOOT_COMMANDS)?;

        write_bytes_to_fat(&root, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&boot_dir, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&root, "boot.cmd", AARCH64_UBOOT_COMMANDS)?;
        write_file_from_host(&root, "u-boot.bin", args.uboot_binary_source_path)?;
        write_file_from_host(&boot_dir, "u-boot.bin", args.uboot_binary_source_path)?;
        write_file_from_host(&kernel_dir, "init.elf", args.init_elf_source_path)?;
        write_file_from_host(&kernel_dir, "kernel.img", args.kernel_image_source_path)?;
    }
    fs.unmount().context("unmount FAT filesystem")?;

    if args.verbose {
        eprintln!("[img] created U-Boot volume: {}", args.img_path);
    }
    Ok(())
}

fn write_mbr(file: &mut File, image_size_bytes: u64) -> Result<()> {
    let mbr = make_mbr(image_size_bytes)?;
    file.seek(SeekFrom::Start(0)).context("seek to MBR")?;
    file.write_all(&mbr).context("write MBR")?;
    file.flush().context("flush MBR")?;
    Ok(())
}

fn make_mbr(image_size_bytes: u64) -> Result<[u8; DISK_SECTOR_SIZE as usize]> {
    if image_size_bytes % DISK_SECTOR_SIZE != 0 {
        anyhow::bail!("U-Boot image size must be sector-aligned");
    }

    let total_sectors = image_size_bytes / DISK_SECTOR_SIZE;
    let partition_sectors = total_sectors
        .checked_sub(BOOT_PARTITION_START_SECTOR)
        .context("U-Boot image is too small for its boot partition")?;
    let partition_start = u32::try_from(BOOT_PARTITION_START_SECTOR)
        .context("boot partition offset does not fit in an MBR")?;
    let partition_sectors =
        u32::try_from(partition_sectors).context("boot partition does not fit in an MBR")?;

    let mut mbr = [0u8; DISK_SECTOR_SIZE as usize];
    let entry = &mut mbr[446..462];
    entry[0] = 0x80; // Active/bootable.
    entry[1..4].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[4] = 0x0c; // FAT32 with LBA addressing.
    entry[5..8].copy_from_slice(&[0xfe, 0xff, 0xff]);
    entry[8..12].copy_from_slice(&partition_start.to_le_bytes());
    entry[12..16].copy_from_slice(&partition_sectors.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    Ok(mbr)
}

fn make_uboot_script_image(script: &[u8]) -> Result<Vec<u8>> {
    const HEADER_SIZE: usize = 64;
    const UBOOT_MAGIC: u32 = 0x2705_1956;
    const OS_LINUX: u8 = 5;
    const ARCH_ARM64: u8 = 22;
    const TYPE_SCRIPT: u8 = 6;
    const COMPRESSION_NONE: u8 = 0;

    // Legacy U-Boot scripts use the multi-image payload layout: a big-endian
    // component-size table, its zero terminator, then the script itself.
    let script_size = u32::try_from(script.len()).context("U-Boot script is too large")?;
    let mut payload = Vec::with_capacity(8 + script.len());
    payload.extend_from_slice(&script_size.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(script);

    let payload_size = u32::try_from(payload.len()).context("U-Boot script image is too large")?;
    let mut header = [0u8; HEADER_SIZE];
    write_be_u32(&mut header[0..4], UBOOT_MAGIC);
    write_be_u32(&mut header[12..16], payload_size);
    write_be_u32(&mut header[24..28], crc32(&payload));
    header[28] = OS_LINUX;
    header[29] = ARCH_ARM64;
    header[30] = TYPE_SCRIPT;
    header[31] = COMPRESSION_NONE;
    let name = b"A9N aarch64 U-Boot";
    header[32..32 + name.len()].copy_from_slice(name);
    let header_crc = crc32(&header);
    write_be_u32(&mut header[4..8], header_crc);

    let mut image = Vec::with_capacity(HEADER_SIZE + payload.len());
    image.extend_from_slice(&header);
    image.extend_from_slice(&payload);
    Ok(image)
}

fn write_be_u32(destination: &mut [u8], value: u32) {
    destination.copy_from_slice(&value.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn ensure_dir<'a, T: fatfs::ReadWriteSeek + 'a>(
    parent: &'a fatfs::Dir<T>,
    name: &str,
) -> Result<fatfs::Dir<'a, T>> {
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

fn write_file_from_host<T: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<T>,
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

fn write_bytes_to_fat<T: fatfs::ReadWriteSeek>(
    dir: &fatfs::Dir<T>,
    file_name: &str,
    contents: &[u8],
) -> Result<()> {
    if dir.open_file(file_name).is_ok() {
        dir.remove(file_name)
            .with_context(|| format!("remove existing file: {}", file_name))?;
    }
    let mut fat_file = dir
        .create_file(file_name)
        .with_context(|| format!("create fat file: {}", file_name))?;
    fat_file.write_all(contents)?;
    fat_file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uboot_disk_has_bootable_fat32_partition() {
        let image_size = 64 * 1024 * 1024;
        let mbr = make_mbr(image_size).expect("make MBR");
        let entry = &mbr[446..462];

        assert_eq!(entry[0], 0x80);
        assert_eq!(entry[4], 0x0c);
        assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()), 2048);
        assert_eq!(
            u32::from_le_bytes(entry[12..16].try_into().unwrap()),
            129_024
        );
        assert_eq!(&mbr[510..512], &[0x55, 0xaa]);
    }

    #[test]
    fn uboot_script_uses_legacy_multi_image_payload() {
        let script = b"echo A9N\n";
        let image = make_uboot_script_image(script).expect("make script image");
        let payload = &image[64..];

        assert_eq!(&image[0..4], &0x2705_1956u32.to_be_bytes());
        assert_eq!(
            u32::from_be_bytes(image[12..16].try_into().unwrap()) as usize,
            payload.len()
        );
        assert_eq!(
            u32::from_be_bytes(image[24..28].try_into().unwrap()),
            crc32(payload)
        );
        assert_eq!(
            u32::from_be_bytes(payload[0..4].try_into().unwrap()) as usize,
            script.len()
        );
        assert_eq!(&payload[4..8], &[0, 0, 0, 0]);
        assert_eq!(&payload[8..], script);

        let mut header = <[u8; 64]>::try_from(&image[..64]).unwrap();
        let recorded_crc = u32::from_be_bytes(header[4..8].try_into().unwrap());
        header[4..8].fill(0);
        assert_eq!(recorded_crc, crc32(&header));
    }
}
