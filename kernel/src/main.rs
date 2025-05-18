#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use vga::println;

mod gdt;
mod interrupts;
mod panic;
mod vga;

/// The entrypoint into the kernel. Do NOT call this function directly. It gets
/// invoked automatically by the bootloader after setting up the stack and
/// performing necessary configuration.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    main();

    // If the main function exits, just halt the CPU
    loop {
        x86_64::instructions::hlt();
    }
}

fn main() {
    gdt::init_gdt();
    interrupts::init_idt();

    println!("Hello World!");
}
