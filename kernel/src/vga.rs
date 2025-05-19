//! This module contains the VGA text mode driver used to print to the screen
//! before we have a graphical environment

use spin::Mutex;
use volatile::Volatile;

struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

pub const BUFFER_HEIGHT: usize = 25;
pub const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> Self {
        Self((background as u8) << 4 | (foreground as u8))
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code: self.color_code,
                });
                self.column_position += 1;
            }
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // printable ASCII byte or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // not part of printable ASCII range
                _ => self.write_byte(0xfe),
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }

        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };

        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
}

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

lazy_static::lazy_static! {
    static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::White, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    });
}

#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments) {
    use core::fmt::Write;

    // We have to disable interrupts during this call to allow interrupt handles
    // to print to the screen
    x86_64::instructions::interrupts::without_interrupts(|| {
        // NOTE: our VGA write implementation is infallible
        WRITER.lock().write_fmt(args).unwrap();
    });
}

/// Changes the current color code of the VGA writer
pub fn set_color_code(color: ColorCode) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER.lock().color_code = color;
    });
}

/// Executes the given function with the provided color code. This function can
/// be nested
pub fn with_color<F: FnOnce() -> R, R>(foreground: Color, f: F) -> R {
    let mut color_code = ColorCode::new(foreground, Color::Black);

    // FIXME: is this usage of without_interrupts correct?

    x86_64::instructions::interrupts::without_interrupts(|| {
        core::mem::swap(&mut WRITER.lock().color_code, &mut color_code);
    });

    let res = f();

    x86_64::instructions::interrupts::without_interrupts(|| {
        core::mem::swap(&mut WRITER.lock().color_code, &mut color_code);
    });

    res
}

macro_rules! print {
    ($($arg:tt)*) => ($crate::vga::_print(format_args!($($arg)*)));
}
pub(crate) use print;

macro_rules! println {
    () => ($crate::vga::print!("\n"));
    ($($arg:tt)*) => ($crate::vga::print!("{}\n", format_args!($($arg)*)));
}
pub(crate) use println;
use x86_64::instructions::port::Port;

/// Moves the cursor on the current line
pub fn set_column_position(position: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        WRITER.lock().column_position = (position as usize).min(BUFFER_WIDTH)
    });
}

const VGA_CMD_PORT: u16 = 0x3D4;
const VGA_DATA_PORT: u16 = 0x3D5;

/// Moves the cursor on the current line
pub fn set_cursor_position(x: u8, y: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut cmd_port = Port::<u8>::new(VGA_CMD_PORT);
        let mut data_port = Port::<u8>::new(VGA_DATA_PORT);

        let pos = y as u16 * BUFFER_WIDTH as u16 + x as u16;

        unsafe {
            cmd_port.write(0x0F);
            data_port.write((pos & 0xFF) as u8);
            cmd_port.write(0x0E);
            data_port.write(((pos >> 8) & 0xFF) as u8);
        }
    });
}

pub fn enable_cursor(start: u8, end: u8) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut cmd_port = Port::<u8>::new(VGA_CMD_PORT);
        let mut data_port = Port::<u8>::new(VGA_DATA_PORT);

        unsafe {
            cmd_port.write(0x0A);
            let s = data_port.read();
            data_port.write((s & 0xC0) | start);

            cmd_port.write(0x0B);
            let e = data_port.read();
            data_port.write((e & 0xE0) | end);
        }
    });
}

pub fn disable_cursor() {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut cmd_port = Port::<u8>::new(VGA_CMD_PORT);
        let mut data_port = Port::<u8>::new(VGA_DATA_PORT);

        unsafe {
            cmd_port.write(0x0A);
            data_port.write(0x20);
        }
    });
}
