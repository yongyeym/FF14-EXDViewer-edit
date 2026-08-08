//! Structs and utilities for parsing .hwc files.

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

/// Width of every cursor, in pixels.
pub const WIDTH: usize = 64;

/// Height of every cursor, in pixels.
pub const HEIGHT: usize = 64;

const SIZE: usize = WIDTH * HEIGHT * 4;

/// A mouse cursor, in the shape the operating system takes for a hardware cursor.
#[derive(Debug)]
pub struct HardwareCursor {
	data: Vec<u8>,
}

impl HardwareCursor {
	/// The cursor's pixels, row major, four bytes each with alpha first.
	pub fn data(&self) -> &[u8] {
		&self.data
	}
}

impl File for HardwareCursor {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut data = Vec::new();
		stream.read_to_end(&mut data)?;
		// The file is a bare pixel buffer, so its length is the only thing that can catch a file
		// that is not one of these.
		if data.len() != SIZE {
			return Err(Error::Invalid(
				ErrorValue::Other("HWC".into()),
				format!("expected {SIZE} bytes, got {}", data.len()),
			));
		}
		Ok(Self { data })
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{HardwareCursor, SIZE};

	#[test]
	fn empty() {
		assert!(matches!(
			HardwareCursor::read(io::empty()),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn a_file_of_the_wrong_length_is_an_error() {
		assert!(matches!(
			HardwareCursor::read(Cursor::new(vec![0; SIZE - 1])),
			Err(Error::Invalid(..))
		));

		assert!(matches!(
			HardwareCursor::read(Cursor::new(vec![0; SIZE + 1])),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn reads_the_whole_image() {
		let bytes = (0..SIZE).map(|index| index as u8).collect::<Vec<_>>();
		let cursor = HardwareCursor::read(Cursor::new(bytes.clone())).unwrap();
		assert_eq!(cursor.data(), bytes);
	}
}
