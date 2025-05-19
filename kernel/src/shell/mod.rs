use futures_util::StreamExt;
use keyboard::ScancodeStream;
use parser::Parser;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts::Us104Key};

use crate::vga::{self, Color, print, println};

pub mod keyboard;
pub mod parser;

const INPUT_BUFFER_LEN: usize = vga::BUFFER_WIDTH - get_prompt().len() - 1;
type InputBuffer = heapless::String<INPUT_BUFFER_LEN>;

pub async fn run() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore);

    let mut history = heapless::Deque::<InputBuffer, 16>::new();

    let mut input_buffer = InputBuffer::new();
    let mut cursor_position = 0u8;

    vga::enable_cursor(13, 15);

    print_prompt();

    while let Some(scancode) = scancodes.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => {
                        // Handle enter
                        if character == '\n' {
                            println!();

                            if parse_and_execute(&input_buffer).await {
                                vga::disable_cursor();
                                return;
                            }

                            print_prompt();

                            // Pop the last item if the history is full and push this command into the queue
                            if history.is_full() {
                                history.pop_back();
                            }
                            history.push_front(input_buffer.clone()).unwrap();

                            input_buffer.clear();
                            cursor_position = 0;
                            continue;
                        }

                        // Handle backspace
                        if character == '\x08' {
                            if keyboard.get_modifiers().is_ctrl() {
                                input_buffer.clear();
                                cursor_position = 0;
                            } else {
                                input_buffer.pop();
                                cursor_position = cursor_position.saturating_sub(1);
                            }

                            let col = get_prompt().len() as u8 + cursor_position;

                            vga::set_column_position(col);
                            for _ in
                                (get_prompt().len() + cursor_position as usize)..vga::BUFFER_WIDTH
                            {
                                print!(" ");
                            }
                            vga::set_column_position(col);

                            vga::set_cursor_position(col, vga::BUFFER_HEIGHT as u8 - 1);

                            continue;
                        }

                        // Handle normal character
                        if input_buffer.push(character).is_ok() {
                            cursor_position += 1;
                            print!("{}", character);

                            let col = get_prompt().len() as u8 + cursor_position;

                            vga::set_cursor_position(col, vga::BUFFER_HEIGHT as u8 - 1);
                        }
                    }
                    DecodedKey::RawKey(_) => {}
                }
            }
        }
    }
}

const fn get_prompt() -> &'static str {
    "root@riptide> "
}

fn print_prompt() {
    let prompt = get_prompt();

    print!("{}", get_prompt());
    vga::set_cursor_position(prompt.len() as u8, vga::BUFFER_HEIGHT as u8 - 1);
}

async fn parse_and_execute(input: &str) -> bool {
    vga::with_color(Color::LightGray, || println!("input: {:?}", input));

    let mut args = heapless::Deque::<&str, 80>::new();

    for token in Parser::new(input) {
        args.push_back(token).ok();
    }

    vga::with_color(Color::LightGray, || println!("args: {:?}", args));

    match args.pop_front() {
        Some("help") => {
            println!("Help message")
        }
        Some("whoami") => {
            println!("root")
        }
        Some("echo" | "print") => {
            let len = args.len();

            for (i, arg) in args.iter().enumerate() {
                print!("{arg}");

                if i < len - 1 {
                    print!(" ");
                }
            }

            println!();
        }
        Some("pwd") => {
            println!("/root");
        }
        Some("uname") => {
            print!("Riptide");

            if let Some(&"-a") = args.front() {
                print!(" riptide {} x86_64", env!("CARGO_PKG_VERSION"));
            }

            println!();
        }
        Some("ls") => todo!("list files"),
        Some("cat") => todo!("read file"),
        Some("touch") => todo!("create file"),
        Some("exit") => {
            return true;
        }
        // Unrecognized command
        Some(cmd) => {
            println!("command not found: {}", cmd)
        }
        // Got no actual input (just whitespace)
        None => {}
    }

    false
}
