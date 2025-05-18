//! This module contains functions related to our operating system's
//! `#[panic_handler]` implementation undefined

use core::panic::PanicInfo;

use crate::vga::{self, Color, ColorCode, print, println};

/// Our function for handling panics within Rust code
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Reset color code in case we were doing something weird
    vga::set_color_code(ColorCode::new(Color::White, Color::Black));

    /* Create a separator to print panic information */

    vga::with_color(Color::DarkGray, || {
        println!();
        for _ in 0..60 {
            print!("=");
        }
        println!();
        println!();
    });

    /* Print the location of the panic */

    vga::with_color(Color::LightRed, || print!("ERROR: "));
    print!("kernel panicked ");

    vga::with_color(Color::LightGray, || match info.location() {
        Some(loc) => print!("(at {})", loc),
        None => print!("(at <unspecified>)"),
    });
    println!(":");
    println!();

    /* Print the panic message itself */

    vga::with_color(Color::LightGray, || println!("{}", info.message()));

    /* Hang the processor */

    loop {
        x86_64::instructions::hlt();
    }
}
