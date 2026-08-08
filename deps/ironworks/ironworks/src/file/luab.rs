//! Structs and utilities for parsing .luab files.

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

/// The signature Lua writes at the head of a compiled chunk.
const SIGNATURE: [u8; 4] = [0x1B, b'L', b'u', b'a'];

/// A compiled Lua chunk, in the form `luac` writes it. The game wraps it in nothing, so reading
/// past the signature is a matter for a Lua implementation.
#[derive(Debug)]
pub struct LuaBytecode {
	data: Vec<u8>,
}

impl LuaBytecode {
	/// The chunk, signature included.
	pub fn data(&self) -> &[u8] {
		&self.data
	}
}

impl File for LuaBytecode {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut data = Vec::new();
		stream.read_to_end(&mut data)?;
		if !data.starts_with(&SIGNATURE) {
			return Err(Error::Invalid(
				ErrorValue::Other("LUAB".into()),
				"missing Lua signature".into(),
			));
		}
		Ok(Self { data })
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{LuaBytecode, SIGNATURE};

	#[test]
	fn empty() {
		assert!(matches!(
			LuaBytecode::read(io::empty()),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn a_chunk_without_the_signature_is_an_error() {
		assert!(matches!(
			LuaBytecode::read(Cursor::new(b"Lua\0".to_vec())),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn reads_the_whole_chunk() {
		let mut bytes = SIGNATURE.to_vec();
		bytes.extend([0x51, 0, 1, 4, 4, 4, 8, 0]);
		let chunk = LuaBytecode::read(Cursor::new(bytes.clone())).unwrap();
		assert_eq!(chunk.data(), bytes);
	}
}
