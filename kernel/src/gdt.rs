use static_cell::StaticCell;
use x86_64::{
    VirtAddr,
    registers::segmentation::Segment,
    structures::{
        gdt::{Descriptor, GlobalDescriptorTable},
        tss::TaskStateSegment,
    },
};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Initializes the Global Descriptor Table (GDT) and the Task State Segment
/// (TSS). Must only be called once during initialization to prevent a panic.
#[allow(clippy::let_and_return)]
pub fn init_gdt() {
    static GDT: StaticCell<GlobalDescriptorTable> = StaticCell::new();
    static TSS: StaticCell<TaskStateSegment> = StaticCell::new();

    /* Init GDT */

    let gdt = GDT
        .try_init(GlobalDescriptorTable::new())
        .expect("Tried to initialize GDT more than once");

    let code_segment = gdt.append(Descriptor::kernel_code_segment());

    /* Init TSS */

    let tss = TSS
        .try_init(TaskStateSegment::new())
        .expect("Tried to initialize TSS more than once");

    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
        const STACK_SIZE: usize = 4096 * 5;
        static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

        let stack_start = VirtAddr::from_ptr(&raw const STACK);
        let stack_end = stack_start + STACK_SIZE as u64;

        stack_end
    };

    let tss_segment = gdt.append(Descriptor::tss_segment(tss));

    /* Load GDT and TSS */

    gdt.load();

    unsafe {
        x86_64::registers::segmentation::CS::set_reg(code_segment);
        x86_64::instructions::tables::load_tss(tss_segment);
    }
}
