# SPENCER - System Provisioning Engine for Nun Components & Embedded Runtime

<p align="center">
  <img src="./resources/rec.gif" alt="Terminal running SPENCER" width="800"/>
</p>

SPENCER is a comprehensive OS construction system that integrates the A9N Microkernel, Nun OS Runtime, and architecture-appropriate boot software.

It automatically generates executable OS images, significantly simplifying the development of embedded systems based on A9N.

## Architecture Overview

- [**A9N Microkernel**](https://github.com/horizon2038/A9n): Capability-based 3rd-generation microkernel

- [**Nun OS Framework**](https://github.com/horizon2038/Nun): OS runtime framework for building embedded operating systems on top of the A9N microkernel

- [**A9NLoader-rs**](https://github.com/horizon2038/a9nloader-rs): x86_64 UEFI bootloader for A9N-based systems
- **U-Boot**: aarch64 secondary bootloader

SPENCER ties these together using a single build interface (`cargo xtask`),
producing a bootable image automatically.

## Build

```bash
cargo xtask build \
    --arch {ARCH, e.g., x86-64} \
    --platform {PLATFORM, e.g., qemu} \
    --{release|debug}
```

For example, the AArch64 QEMU build is:

```bash
cargo xtask build --arch aarch64 --platform qemu --release
```

This builds A9N, the Nun OS payload, and U-Boot, then creates:

```text
out/aarch64-qemu-release/
├── a9n/
│   ├── kernel.elf
│   └── kernel.img
├── u-boot/
│   └── u-boot.bin
└── spencer.img
```

The AArch64 `spencer.img` contains U-Boot legacy boot scripts, `u-boot.bin`,
`kernel.img`, and the Init ELF. QEMU `virt` still receives the generated
`u-boot.bin` separately as firmware because firmware loading and block-device
loading are distinct QEMU interfaces.

### Running with QEMU

```bash
cargo xtask run \
    --arch {ARCH, e.g., x86-64} \
    --platform qemu \
    --{release|debug}
```

The default run exposes one virtual CPU. A multi-core A9N run requires both the
Kernel build switch and the QEMU CPU count:

```bash
cargo xtask run \
    --arch x86-64 \
    --platform qemu \
    --release \
    --enable-smp \
    --smp 4
```

`--enable-smp` configures A9N with `A9N_CONFIG_ENABLE_SMP=ON`. `--smp <N>`
sets QEMU's virtual CPU count and defaults to 1. A value greater than 1 is
rejected unless `--enable-smp` is also present.

Hardware acceleration is controlled with:

```text
--accel auto|on|off
```

The default `auto` mode uses the host's QEMU hardware accelerator when the host
and guest architectures match, and otherwise uses TCG. `on` requires a usable
hardware accelerator and reports an error when none is available. `off` always
uses TCG. Spencer does not retry with TCG when a selected hardware accelerator
fails; rerun with `--accel off` to fall back explicitly.

### U-Boot source and toolchain

For an AArch64 build, SPENCER resolves U-Boot in the following order:

1. `UBOOT_BIN`, as an explicit prebuilt-binary override.
2. `UBOOT_SOURCE`, as an explicit U-Boot source tree.
3. `tools/u-boot`, when it is a source tree or contains `u-boot.bin`.
4. A shallow checkout of U-Boot `v2025.10` from the official repository.

Source builds use `qemu_arm64_defconfig` and an out-of-tree build directory.
Set `UBOOT_CROSS_COMPILE` to the AArch64 compiler prefix when it cannot be
detected automatically:

```bash
UBOOT_SOURCE=/path/to/u-boot \
UBOOT_CROSS_COMPILE=aarch64-linux-gnu- \
cargo xtask build --arch aarch64 --platform qemu --release
```

`UBOOT_MAKE` overrides the Make executable, and `UBOOT_JOBS` overrides the
parallel job count. AArch64 Linux hosts can build natively; other hosts need an
AArch64 GCC cross compiler or may provide a prebuilt binary through
`UBOOT_BIN`.

### Debugging with GDB
```bash
cargo xtask gdb \
    --arch {ARCH, e.g., x86-64} \
    --platform qemu \
    --{release|debug} \
    --gdb --stop
```

## Advanced Usage

### Injecting an external OS payload

By default, SPENCER builds the template OS payload from `./core/Cargo.toml`
and installs the produced `core` binary as `/kernel/init.elf` in the generated
FAT image.

For projects that use SPENCER as a build/image/run toolchain, the OS payload
can be supplied from outside the SPENCER repository:

```bash
cargo xtask build \
    --arch x86-64 \
    --platform qemu \
    --release \
    --os-manifest /path/to/os/Cargo.toml \
    --os-target-json /path/to/x86_64-unknown-a9n.json \
    --os-binary my_os
```

The same options are accepted by `cargo xtask run`:

```bash
cargo xtask run \
    --arch x86-64 \
    --platform qemu \
    --release \
    --os-manifest /path/to/os/Cargo.toml \
    --os-target-json /path/to/x86_64-unknown-a9n.json \
    --os-binary my_os
```

Options:

- `--os-manifest`: Cargo manifest of the external OS payload. If omitted,
  SPENCER uses `./core/Cargo.toml`.
- `--os-target-json`: custom Rust target JSON for the OS payload. If omitted,
  SPENCER uses `./Nun/arch/<arch>-unknown-a9n.json`.
- `--os-binary`: output binary name produced by the OS payload package. If
  omitted, SPENCER uses `core`.

The external payload is built into SPENCER's normal output directory:

```text
out/<arch>-<platform>-<profile>/nun_os_target_dir/<target>/<profile>/<os-binary>
```

The image builder then writes that binary directly to:

```text
/kernel/init.elf
```

No post-build image patching is required.

If the custom target JSON uses linker script paths in `pre-link-args`, prefer
absolute paths or paths that are valid from the external payload build context.
This avoids depending on Cargo/rust-lld's current working directory.

## Supported Architectures and Platforms

Currently supported architectures and platforms include:

### Architectures

- `x86_64`
- `aarch64`

### Platforms

- `qemu` (`pc99` for x86_64, `virt` for aarch64)

### Planned Support

- `riscv64` (QEMU, real hardware)

- Non-specific embedded platforms

## License

[MIT License](https://choosealicense.com/licenses/mit/)
