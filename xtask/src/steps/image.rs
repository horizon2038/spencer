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

pub struct BuildRpi4bImgArgs<'a> {
    pub img_path: &'a Utf8Path,
    pub firmware_boot_dir: &'a Utf8Path,
    pub uboot_binary_source_path: &'a Utf8Path,
    pub init_elf_source_path: &'a Utf8Path,
    pub kernel_image_source_path: &'a Utf8Path,
    pub image_size_mib: u64,
    pub verbose: bool,
    pub dry_run: bool,
}

const QEMU_AARCH64_UBOOT_COMMANDS: &[u8] = br#"echo Booting A9N through U-Boot...
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

const RPI4B_UBOOT_COMMANDS: &[u8] = br#"echo Booting A9N on Raspberry Pi 4 through U-Boot...
if test -z "${devtype}"; then setenv devtype mmc; fi
if test -z "${devnum}"; then setenv devnum 0; fi
if test -z "${distro_bootpart}"; then setenv distro_bootpart 1; fi
setenv kernel_addr_r 0x00080000
load ${devtype} ${devnum}:${distro_bootpart} ${kernel_addr_r} /kernel/kernel.img
setexpr a9n_image_size_addr ${kernel_addr_r} + 0x10
setexpr.l a9n_kernel_size *${a9n_image_size_addr}
setexpr ramdisk_addr_r ${kernel_addr_r} + ${a9n_kernel_size}
setexpr ramdisk_addr_r ${ramdisk_addr_r} + 0x200000
load ${devtype} ${devnum}:${distro_bootpart} ${ramdisk_addr_r} /kernel/init.elf
setenv ramdisk_size ${filesize}
setexpr a9n_fdt_destination ${ramdisk_addr_r} + ${ramdisk_size}
setexpr a9n_fdt_destination ${a9n_fdt_destination} + 0x10000
if test -n "${fdt_addr}"; then setenv a9n_fdt_source ${fdt_addr}; else setenv a9n_fdt_source ${fdtcontroladdr}; fi
fdt addr ${a9n_fdt_source}
fdt header get a9n_fdt_size totalsize
setexpr a9n_fdt_capacity ${a9n_fdt_size} + 0x10000
fdt move ${a9n_fdt_source} ${a9n_fdt_destination} ${a9n_fdt_capacity}
booti ${kernel_addr_r} ${ramdisk_addr_r}:${ramdisk_size} ${a9n_fdt_destination}
"#;

const RPI4B_CONFIG_TXT: &[u8] = br#"[all]
arm_64bit=1
kernel=kernel8.img
kernel_address=0x80000
device_tree=bcm2711-rpi-4-b.dtb
enable_uart=1
uart_2ndstage=1
init_uart_clock=48000000
init_uart_baud=115200
enable_gic=1
dtoverlay=disable-bt
disable_overscan=1
"#;

const DISK_SECTOR_SIZE: u64 = 512;
const BOOT_PARTITION_START_SECTOR: u64 = 2048;
const FAT_BPB_HIDDEN_SECTORS_OFFSET: u64 = 28;
const FAT32_BACKUP_BOOT_SECTOR: u64 = 6;

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
    write_fat_hidden_sectors(args.img_path)?;

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
        let boot_script = make_uboot_script_image(QEMU_AARCH64_UBOOT_COMMANDS)?;

        write_bytes_to_fat(&root, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&boot_dir, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&root, "boot.cmd", QEMU_AARCH64_UBOOT_COMMANDS)?;
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

pub fn build_rpi4b_img(args: &BuildRpi4bImgArgs) -> Result<()> {
    if args.dry_run {
        eprintln!("[dry-run] create Raspberry Pi 4 img: {}", args.img_path);
        eprintln!("[dry-run]   /config.txt                 <- generated Raspberry Pi config");
        eprintln!(
            "[dry-run]   /kernel8.img                <- {}",
            args.uboot_binary_source_path
        );
        eprintln!(
            "[dry-run]   Raspberry Pi boot firmware <- {}",
            args.firmware_boot_dir
        );
        eprintln!("[dry-run]   /boot.scr                   <- generated U-Boot script image");
        eprintln!(
            "[dry-run]   /kernel/init.elf            <- {}",
            args.init_elf_source_path
        );
        eprintln!(
            "[dry-run]   /kernel/kernel.img          <- {}",
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
        anyhow::bail!("Raspberry Pi image must be larger than the 1 MiB partition offset");
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
            .context("open Raspberry Pi boot partition for format")?;
        fatfs::format_volume(
            BufStream::new(partition),
            fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32),
        )
        .context("format Raspberry Pi FAT boot volume")?;
    }
    write_fat_hidden_sectors(args.img_path)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(args.img_path.as_std_path())
        .with_context(|| format!("open img for fs: {}", args.img_path))?;
    let partition = StreamSlice::new(file, partition_start, image_size_bytes)
        .context("open Raspberry Pi boot partition")?;
    let fs = fatfs::FileSystem::new(BufStream::new(partition), fatfs::FsOptions::new())
        .context("open Raspberry Pi FAT boot filesystem")?;
    {
        let root = fs.root_dir();
        let boot_dir = ensure_dir(&root, "boot")?;
        let kernel_dir = ensure_dir(&root, "kernel")?;
        let overlays_dir = ensure_dir(&root, "overlays")?;
        let boot_script = make_uboot_script_image(RPI4B_UBOOT_COMMANDS)?;

        write_bytes_to_fat(&root, "config.txt", RPI4B_CONFIG_TXT)?;
        write_bytes_to_fat(&root, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&boot_dir, "boot.scr", &boot_script)?;
        write_bytes_to_fat(&root, "boot.cmd", RPI4B_UBOOT_COMMANDS)?;
        write_file_from_host(&root, "kernel8.img", args.uboot_binary_source_path)?;
        write_file_from_host(&kernel_dir, "init.elf", args.init_elf_source_path)?;
        write_file_from_host(&kernel_dir, "kernel.img", args.kernel_image_source_path)?;

        for file_name in [
            "bootcode.bin",
            "start4.elf",
            "fixup4.dat",
            "bcm2711-rpi-4-b.dtb",
            "LICENCE.broadcom",
            "COPYING.linux",
        ] {
            write_file_from_host(&root, file_name, &args.firmware_boot_dir.join(file_name))?;
        }
        write_file_from_host(
            &overlays_dir,
            "disable-bt.dtbo",
            &args.firmware_boot_dir.join("overlays/disable-bt.dtbo"),
        )?;
    }
    fs.unmount()
        .context("unmount Raspberry Pi FAT boot filesystem")?;

    if args.verbose {
        eprintln!("[img] created Raspberry Pi 4 image: {}", args.img_path);
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

fn write_fat_hidden_sectors(image_path: &Utf8Path) -> Result<()> {
    let hidden_sectors = u32::try_from(BOOT_PARTITION_START_SECTOR)
        .context("boot partition offset does not fit in the FAT BPB")?
        .to_le_bytes();
    let partition_start = BOOT_PARTITION_START_SECTOR * DISK_SECTOR_SIZE;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(image_path.as_std_path())
        .with_context(|| format!("open image to update FAT BPB: {}", image_path))?;

    for boot_sector in [0, FAT32_BACKUP_BOOT_SECTOR] {
        let offset =
            partition_start + boot_sector * DISK_SECTOR_SIZE + FAT_BPB_HIDDEN_SECTORS_OFFSET;
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("seek to FAT BPB hidden-sectors field at {}", offset))?;
        file.write_all(&hidden_sectors)
            .context("write FAT BPB hidden-sectors field")?;
    }
    file.flush().context("flush FAT BPB updates")?;
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

    #[test]
    fn rpi4b_boot_configuration_preserves_the_firmware_dtb() {
        let config = std::str::from_utf8(RPI4B_CONFIG_TXT).unwrap();
        let script = std::str::from_utf8(RPI4B_UBOOT_COMMANDS).unwrap();

        assert!(config.contains("kernel=kernel8.img"));
        assert!(config.contains("kernel_address=0x80000"));
        assert!(config.contains("device_tree=bcm2711-rpi-4-b.dtb"));
        assert!(config.contains("init_uart_clock=48000000"));
        assert!(config.contains("init_uart_baud=115200"));
        assert!(config.contains("dtoverlay=disable-bt"));
        assert!(config.contains("enable_gic=1"));
        assert!(script.contains("setenv kernel_addr_r 0x00080000"));
        assert!(script.contains("${fdt_addr}"));
        assert!(script.contains("a9n_fdt_destination"));
        assert!(script.contains("booti ${kernel_addr_r}"));
    }

    #[test]
    fn rpi4b_image_contains_the_complete_boot_chain() {
        let temporary =
            std::env::temp_dir().join(format!("spencer-rpi4b-image-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        let firmware = temporary.join("firmware");
        std::fs::create_dir_all(&firmware).expect("create firmware fixture");

        for file in [
            "bootcode.bin",
            "start4.elf",
            "fixup4.dat",
            "bcm2711-rpi-4-b.dtb",
            "LICENCE.broadcom",
            "COPYING.linux",
        ] {
            std::fs::write(firmware.join(file), file.as_bytes()).expect("write firmware file");
        }
        std::fs::create_dir_all(firmware.join("overlays"))
            .expect("create firmware overlay fixture directory");
        std::fs::write(firmware.join("overlays/disable-bt.dtbo"), b"disable-bt")
            .expect("write firmware overlay fixture");

        let uboot = temporary.join("u-boot.bin");
        let init = temporary.join("init.elf");
        let kernel = temporary.join("kernel.img");
        std::fs::write(&uboot, b"u-boot").expect("write U-Boot fixture");
        std::fs::write(&init, b"init").expect("write init fixture");
        std::fs::write(&kernel, b"kernel").expect("write kernel fixture");

        let image_path = temporary.join("spencer.img");
        let image_path = camino::Utf8PathBuf::from_path_buf(image_path).expect("UTF-8 image path");
        let firmware = camino::Utf8PathBuf::from_path_buf(firmware).expect("UTF-8 firmware path");
        let uboot = camino::Utf8PathBuf::from_path_buf(uboot).expect("UTF-8 U-Boot path");
        let init = camino::Utf8PathBuf::from_path_buf(init).expect("UTF-8 init path");
        let kernel = camino::Utf8PathBuf::from_path_buf(kernel).expect("UTF-8 kernel path");

        build_rpi4b_img(&BuildRpi4bImgArgs {
            img_path: &image_path,
            firmware_boot_dir: &firmware,
            uboot_binary_source_path: &uboot,
            init_elf_source_path: &init,
            kernel_image_source_path: &kernel,
            image_size_mib: 64,
            verbose: false,
            dry_run: false,
        })
        .expect("build Raspberry Pi image");

        let mut image = OpenOptions::new()
            .read(true)
            .write(true)
            .open(image_path.as_std_path())
            .expect("open image");
        for boot_sector in [0, FAT32_BACKUP_BOOT_SECTOR] {
            let offset = BOOT_PARTITION_START_SECTOR * DISK_SECTOR_SIZE
                + boot_sector * DISK_SECTOR_SIZE
                + FAT_BPB_HIDDEN_SECTORS_OFFSET;
            image
                .seek(SeekFrom::Start(offset))
                .expect("seek to hidden-sectors field");
            let mut hidden_sectors = [0u8; 4];
            image
                .read_exact(&mut hidden_sectors)
                .expect("read hidden-sectors field");
            assert_eq!(
                u32::from_le_bytes(hidden_sectors),
                BOOT_PARTITION_START_SECTOR as u32
            );
        }
        let partition = StreamSlice::new(
            image,
            BOOT_PARTITION_START_SECTOR * DISK_SECTOR_SIZE,
            64 * 1024 * 1024,
        )
        .expect("open partition");
        let fs = fatfs::FileSystem::new(BufStream::new(partition), fatfs::FsOptions::new())
            .expect("open filesystem");
        {
            let root = fs.root_dir();
            for file in [
                "config.txt",
                "kernel8.img",
                "start4.elf",
                "fixup4.dat",
                "bcm2711-rpi-4-b.dtb",
                "boot.scr",
            ] {
                root.open_file(file).expect("boot-chain file exists");
            }
            root.open_dir("kernel")
                .expect("kernel directory")
                .open_file("kernel.img")
                .expect("A9N image exists");
            root.open_dir("overlays")
                .expect("overlays directory")
                .open_file("disable-bt.dtbo")
                .expect("disable-bt overlay exists");
        }
        fs.unmount().expect("unmount image");
        std::fs::remove_dir_all(temporary).expect("remove image fixture");
    }
}
