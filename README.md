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

