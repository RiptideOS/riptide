#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(str_from_raw_parts)]

extern crate alloc;

use bootloader::BootInfo;
use memory::BootInfoFrameAllocator;
use task::{Task, executor::Executor};
use vga::println;
use x86_64::VirtAddr;

mod allocator;
mod device;
mod drivers;
mod fs;
mod gdt;
mod interrupts;
mod memory;
mod panic;
mod shell;
mod task;
mod util;
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

    drivers::char::init().expect("failed to init char dev drivers");
    fs::init();

    let mut executor = Executor::new();
    executor.spawn(Task::new(shell::run()));
    executor.run();
}
