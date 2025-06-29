use alloc::{collections::vec_deque::VecDeque, format, string::String, vec::Vec};

use futures_util::StreamExt;
use keyboard::ScancodeStream;
use parser::Parser;
use pc_keyboard::{DecodedKey, HandleControl, Keyboard, ScancodeSet1, layouts::Us104Key};

use crate::{
    fs::{
        FileMode, FsNodeKind,
        vfs::{self, DirectoryEntry, DirectoryIterationEntry, IoError},
    },
    vga::{self, Color, print, println},
};

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

    let mut args = VecDeque::<&str>::new();

    for token in Parser::new(input) {
        args.push_back(token);
    }

    vga::with_color(Color::LightGray, || println!("args: {:?}", args));

    loop {
        match args.pop_front() {
            Some("help") => {
                println!("TODO: insert a help message here")
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
                println!("/");
            }
            Some("uname") => {
                print!("Riptide");

                if let Some(&"-a") = args.front() {
                    print!(" riptide {} x86_64", env!("CARGO_PKG_VERSION"));
                }

                println!();
            }
            Some("ls") => {
                let args = args.make_contiguous();

                let path = without_flags(args).last().cloned().unwrap_or("/"); // FIXME: use pwd

                let all = has_boolean_option(args, 'a');
                let long = has_boolean_option(args, 'l');
                let human_readable = has_boolean_option(args, 'h');
                let show_node_ids = has_boolean_option(args, 'i');

                let e = match vfs::get().stat(path) {
                    Ok(e) => e,
                    Err(IoError::EntryNotFound) => {
                        println!("ls: {}: No such file or directory", path);
                        break;
                    }
                    Err(_) => todo!(),
                };

                let format_entry_short = |entry: &DirectoryIterationEntry| {
                    if show_node_ids {
                        print!("{} ", entry.id.as_u64());
                    }

                    vga::with_color(entry.kind.color_code(), || println!("{}", entry.name));
                };

                let format_entry_long = |entry: &DirectoryEntry| {
                    if show_node_ids {
                        print!("{} ", entry.node.id.as_u64());
                    }

                    let meta = entry.node.metadata.lock();

                    println!(
                        "{}rw-r--r--@ 1 root root {:>3} {:>2} {}",
                        entry.node.kind, meta.size, meta.modified_at, entry.name
                    );
                };

                if e.node.is_directory() {
                    let entries = match vfs::get().read_directory(path) {
                        Ok(v) => v,
                        Err(_) => todo!(),
                    };

                    for child in entries {
                        if long {
                            // FIXME: create a path join abstraction

                            let child_path = if e.name.as_ref() == "/" {
                                format!("/{}", child.name)
                            } else {
                                format!("{}/{}", e.name, child.name)
                            };

                            let c = vfs::get().stat(&child_path).unwrap();

                            format_entry_long(&c);
                        } else {
                            format_entry_short(&child);
                        }
                    }
                } else if long {
                    format_entry_long(&e);
                } else {
                    format_entry_short(&e.as_ref().into());
                }
            }
            Some("cat") => {
                let Some(path) = args.front() else {
                    println!("error: no path provided");
                    break;
                };

                let f = vfs::get().open(path, FileMode::Read).unwrap();

                let mut data = [0u8; 512];

                let bytes = vfs::get().read(f, &mut data).unwrap();

                let data = &data[..bytes];

                println!("{}", String::from_utf8_lossy(data));
            }
            Some("touch") => {
                let Some(path) = args.front() else {
                    println!("error: no path provided");
                    break;
                };

                let f = vfs::get().open(path, FileMode::Write).unwrap();
                vfs::get().close(f).unwrap();
            }
            Some("mkdir") => {
                let args = args.make_contiguous();

                let Some(path) = without_flags(args).last().cloned() else {
                    println!("error: no path provided");
                    break;
                };

                match vfs::get().create_directory(path) {
                    Ok(_) => {}
                    Err(e) => panic!("{e:?}"),
                }
            }
            Some("rm") => println!("error: not implemented yet"),
            Some("realpath") => println!("error: not implemented yet"),
            Some("basename") => println!("error: not implemented yet"),
            Some("cd") => println!("error: not implemented yet"),
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

        break;
    }

    false
}

/// Parses argument list for single character option flags
fn has_boolean_option(args: &[&str], flag: char) -> bool {
    for arg in args {
        if !arg.starts_with("-") {
            continue;
        }

        if arg.starts_with("--") {
            todo!("parse named arguments");
        }

        for c in arg.chars().skip(1) {
            if c == flag {
                return true;
            }
        }
    }

    false
}

fn without_flags<'a>(args: &[&'a str]) -> Vec<&'a str> {
    args.iter()
        .filter(|a| !a.starts_with("-"))
        .cloned()
        .collect()
}
