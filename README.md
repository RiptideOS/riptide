# RiptideOS

This is the main repository for the RiptideOS project. Riptide is a hobby OS written from scratch in Rust. The goal of this project is to create a POSIX compliant kernel and accompanying desktop operating system for the modern era. 

## Running Locally

### Prerequisites

To run RiptideOS locally, you can use [QEMU](https://www.qemu.org/) to emulate an x86 CPU. Install it form [here](https://www.qemu.org/download/). To create a bootable disk image, you will also need [`bootimage`](https://github.com/rust-osdev/bootimage). Install it like so:

```
cargo install bootimage
```

### Launching

Once the prerequisites are installed, you should just be able to run with cargo:

```
cargo run
```