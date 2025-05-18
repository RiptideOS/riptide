#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use bootloader::BootInfo;
use memory::BootInfoFrameAllocator;
use vga::println;
use x86_64::{VirtAddr, structures::paging::Page};

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

    {
        let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
        let mut mapper = unsafe { memory::init(phys_mem_offset) };
        let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

        // map an unused page
        let page = Page::containing_address(VirtAddr::new(0xdeadbeaf000));
        memory::map_vga_text_buffer(page, &mut mapper, &mut frame_allocator);

        // write the string `New!` to the screen through the new mapping
        let page_ptr: *mut u64 = page.start_address().as_mut_ptr();
        unsafe { page_ptr.offset(400).write_volatile(0x_f021_f077_f065_f04e) };
    }

    println!("We survived!");

    loop {
        x86_64::instructions::hlt();
    }
}
