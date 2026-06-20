# SPENCER - System Provisioning Engine for Nun Components & Embedded Runtime

<p align="center">
  <img src="./resources/rec.gif" alt="Terminal running SPENCER" width="800"/>
</p>

SPENCER is a comprehensive OS construction system that integrates the A9N Microkernel, Nun OS Runtime, and A9NLoader.

It automatically generates executable OS images, significantly simplifying the development of embedded systems based on A9N.

## Architecture Overview

- [**A9N Microkernel**](https://github.com/horizon2038/A9n): Capability-based 3rd-generation microkernel

- [**Nun OS Framework**](https://github.com/horizon2038/Nun): OS runtime framework for building embedded operating systems on top of the A9N microkernel

- [**A9NLoader-rs**](https://github.com/horizon2038/a9nloader-rs): Bootloader for A9N-based systems, written in Rust

SPENCER ties these together using a single build interface (`cargo xtask`),
producing a bootable UEFI disk image automatically.

## Build

```bash
cargo xtask build \
    --arch {ARCH, e.g., x86-64} \
    --platform {PLATFORM, e.g., qemu} \
    --{release|debug}
```

### Running with QEMU
```bash
cargo xtask run \
    --arch {ARCH, e.g., x86-64} \
    --platform qemu \
    --{release|debug}
```

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

### Platforms

- `qemu`

### Planned Support

- `aarch64` (QEMU, real hardware)

- `riscv64` (QEMU, real hardware)

- Non-specific embedded platforms

## License

[MIT License](https://choosealicense.com/licenses/mit/)
