use core::str;

pub struct Parser<'source> {
    input: &'source [u8],
    position: usize,
}

impl<'source> Parser<'source> {
    pub fn new(input: &'source str) -> Self {
        assert!(input.is_ascii(), "parser input contained non-ascii text");

        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }
}

impl<'source> Iterator for Parser<'source> {
    type Item = &'source str;

    fn next(&mut self) -> Option<Self::Item> {
        let mut start = self.position;
        let mut parsing_string = false;

        for char in self.input[self.position..].iter() {
            match *char {
                // Start or end of string
                b'"' => {
                    // If we are not already parsing a string, move the start up
                    // by one to point to the inside of the string
                    if !parsing_string {
                        // If we already have some characters, return that so on
                        // the next iteration we start right on the string
                        if self.position > start {
                            // SAFETY: we know this string slice is valid and has
                            // the same lifetime as the input since it points
                            // directly into the input buffer
                            return Some(unsafe {
                                str::from_raw_parts(
                                    self.input.as_ptr().add(start),
                                    self.position - start,
                                )
                            });
                        }

                        start += 1;
                        parsing_string = true;

                        self.position += 1;
                        continue;
                    }

                    // Otherwise, this is the end of a string, so we need to
                    // return the slice from the beginning of the string to the
                    // current char and then move the position up

                    // SAFETY: we know this string slice is valid and has
                    // the same lifetime as the input since it points
                    // directly into the input buffer
                    let ret = unsafe {
                        str::from_raw_parts(self.input.as_ptr().add(start), self.position - start)
                    };
                    self.position += 1;

                    return Some(ret);
                }
                // Whitespace
                b' ' | b'\t' => {
                    // If we are in the middle of parsing a string, just munch
                    // the space.
                    if parsing_string {
                        self.position += 1;
                        continue;
                    }

                    // Otherwise, this is the end of a token so we need to
                    // return the string slice if there is anything in it.

                    // SAFETY: we know this string slice is valid and has
                    // the same lifetime as the input since it points
                    // directly into the input buffer
                    let ret = unsafe {
                        str::from_raw_parts(self.input.as_ptr().add(start), self.position - start)
                    };
                    self.position += 1;

                    if !ret.is_empty() {
                        return Some(ret);
                    } else {
                        start = self.position;
                        continue;
                    }
                }
                // Any other character
                _ => {
                    // Munch the character
                    self.position += 1;
                }
            }
        }

        // We reached the end of the input. If we have any remaining characters,
        // return the buffer. Otherwise return none.

        if self.position > start {
            // SAFETY: we know this string slice is valid and has
            // the same lifetime as the input since it points
            // directly into the input buffer
            Some(unsafe {
                str::from_raw_parts(self.input.as_ptr().add(start), self.position - start)
            })
        } else {
            None
        }
    }
}
