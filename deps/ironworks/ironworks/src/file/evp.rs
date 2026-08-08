//! Structs and utilities for parsing .evp files.

use std::fmt::Debug;

use binrw::{BinRead, binread};

use crate::{FileStream, error::Result};

use super::File;

/// The number of flags a set carries.
pub const FLAG_COUNT: usize = 512;

/// Per-set flags gating equipment VFX, consulted for sets whose .eqp entry sets
/// `uses_vfx_parameter`.
#[binread]
#[br(little, magic = b"EVP")]
pub struct EquipmentVfxParameter {
	#[br(temp)]
	count: u8,

	#[br(count = count)]
	sets: Vec<u16>,

	#[br(count = count)]
	flags: Vec<[u8; FLAG_COUNT]>,
}

impl EquipmentVfxParameter {
	/// The set IDs this file carries, ascending.
	pub fn sets(&self) -> &[u16] {
		&self.sets
	}

	/// Get the flags for a set ID. Bit 0 of a flag applies to the body and bit 1 to the head; what
	/// the position within the array selects has not been identified.
	pub fn flags(&self, set: u16) -> Option<&[u8; FLAG_COUNT]> {
		let index = self.sets.binary_search(&set).ok()?;
		self.flags.get(index)
	}
}

impl File for EquipmentVfxParameter {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

impl Debug for EquipmentVfxParameter {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("EquipmentVfxParameter")
			.field("sets", &self.sets)
			.finish()
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{EquipmentVfxParameter, FLAG_COUNT};

	fn parameters(sets: &[(u16, u8)]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"EVP");
		bytes.push(u8::try_from(sets.len()).unwrap());
		for &(set, _) in sets {
			bytes.extend(set.to_le_bytes());
		}
		for &(_, fill) in sets {
			bytes.extend([fill; FLAG_COUNT]);
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			EquipmentVfxParameter::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn truncated() {
		let mut bytes = parameters(&[(1, 0xaa)]);
		bytes.truncate(bytes.len() - 1);
		assert!(matches!(
			EquipmentVfxParameter::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	/// The flag arrays start past the set IDs, not immediately after the header. Set IDs of 0x1111
	/// and 0x2222 make reading from the wrong base visible.
	#[test]
	fn flags_start_past_the_set_ids() {
		let file =
			EquipmentVfxParameter::read(Cursor::new(parameters(&[(0x1111, 0xaa), (0x2222, 0xbb)])))
				.unwrap();

		assert_eq!(file.sets(), [0x1111, 0x2222]);
		assert_eq!(file.flags(0x1111).unwrap(), &[0xaa; FLAG_COUNT]);
		assert_eq!(file.flags(0x2222).unwrap(), &[0xbb; FLAG_COUNT]);
		assert!(file.flags(0x3333).is_none());
	}
}
