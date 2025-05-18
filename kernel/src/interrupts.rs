use static_cell::StaticCell;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::{gdt, vga::println};

/// Initializes the Interrupt Descriptor Table (IDT). Must only be called once
/// during initialization to prevent a panic.
pub fn init_idt() {
    static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new();

    let idt = IDT
        .try_init(InterruptDescriptorTable::new())
        .expect("Tried to initialize IDT more than once");

    idt.breakpoint.set_handler_fn(breakpoint_handler);

    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }

    idt.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}
