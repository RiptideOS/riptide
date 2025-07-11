use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::str::FromStr;

const MAX_PATH_LENGTH: usize = 4096;

#[derive(Debug, Default)]
pub struct Path {
    segments: Vec<String>,
}

impl Path {
    /// Returns true if this path starts with a "/"
    pub fn is_absolute(&self) -> bool {
        self.segments.first().unwrap() == "/"
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().map(|s| s.as_str())
    }
}

pub enum PathParseError {
    Empty,
    MaxLengthExceeded,
}

impl FromStr for Path {
    type Err = PathParseError;

    fn from_str(mut s: &str) -> Result<Self, Self::Err> {
        if !s.is_ascii() {
            todo!("parse non-ascii paths");
        }

        if s.is_empty() {
            return Err(PathParseError::Empty);
        }

        if s.len() > MAX_PATH_LENGTH {
            return Err(PathParseError::MaxLengthExceeded);
        }

        let mut segments = Vec::new();

        if s.starts_with("/") {
            segments.push("/".into());
            s = &s[1..];
        }

        if !s.is_empty() {
            for segment in s.split("/") {
                segments.push(segment.to_string());
            }
        }

        Ok(Self { segments })
    }
}
