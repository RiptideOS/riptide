#![no_std]
#![no_main]

use vga::println;

mod panic;
mod vga;

/// The entrypoint into the kernel. Do NOT call this function directly. It gets
/// invoked automatically by the bootloader after setting up the stack and
/// performing necessary configuration.
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println!("Hello World!");

    loop {
        x86_64::instructions::hlt();
    }
}
