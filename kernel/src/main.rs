#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use bootloader::BootInfo;
use memory::BootInfoFrameAllocator;
use vga::println;
use x86_64::VirtAddr;

mod allocator;
mod gdt;
mod interrupts;
mod memory;
mod panic;
mod vga;

bootloader::entry_point!(kernel_main);

/// The entrypoint into the kernel. Do NOT call this function directly. It gets
/// invoked automatically by the bootloader after setting up the stack and
/// performing necessary configuration.
fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("RiptideOS (v{})", env!("CARGO_PKG_VERSION"));

    gdt::init();
    interrupts::init_idt();
    interrupts::init_pics();

    x86_64::instructions::interrupts::enable();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    println!("We survived!");

    loop {
        x86_64::instructions::hlt();
    }
}
