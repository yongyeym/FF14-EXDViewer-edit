//! Structs and utilities for parsing .gmp files.

use binrw::BinRead;

use crate::{FileStream, error::Result};

use super::{File, block_table::BlockTable};

/// Visor gimmick metadata, present for the set IDs of head equipment.
#[derive(Debug)]
pub struct GimmickParameter(BlockTable);

impl GimmickParameter {
	/// Get gimmick metadata for the specified set ID.
	pub fn set(&self, id: u16) -> Set {
		Set(bitfield::Set::from_bytes(self.0.entry(id, 0).to_le_bytes()))
	}
}

impl File for GimmickParameter {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(Self(<BlockTable as BinRead>::read(&mut stream)?))
	}
}

/// Gimmick metadata for a specific set.
#[derive(Debug)]
pub struct Set(bitfield::Set);

impl Set {
	/// Whether the visor can be toggled at all.
	pub fn enabled(&self) -> bool {
		self.0.enabled()
	}

	/// Whether toggling the visor is animated.
	pub fn animated(&self) -> bool {
		self.0.animated()
	}

	/// Rotation of a toggled visor, in degrees.
	pub fn rotation(&self) -> [u16; 3] {
		[
			self.0.rotation_a(),
			self.0.rotation_b(),
			self.0.rotation_c(),
		]
	}

	/// Unidentified.
	pub fn unknown_a(&self) -> u8 {
		self.0.unknown_a()
	}

	/// Unidentified.
	pub fn unknown_b(&self) -> u8 {
		self.0.unknown_b()
	}
}

#[allow(dead_code)]
mod bitfield {
	use modular_bitfield::prelude::*;

	#[bitfield]
	#[derive(Debug)]
	pub struct Set {
		pub enabled: bool,
		pub animated: bool,
		pub rotation_a: B10,
		pub rotation_b: B10,
		pub rotation_c: B10,
		pub unknown_a: B4,
		pub unknown_b: B4,
		#[skip]
		_reserved: B24,
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::GimmickParameter;

	/// Build a file carrying only block 0, padded out to its full 160 entries. `control` occupies
	/// entry 0.
	fn parameters(control: u64, entries: &[u64]) -> Vec<u8> {
		let mut entries = entries.to_vec();
		entries.resize(160, 0);
		entries[0] = control;
		entries
			.iter()
			.flat_map(|entry| entry.to_le_bytes())
			.collect()
	}

	#[test]
	fn empty() {
		assert!(matches!(
			GimmickParameter::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn unpacks_rotations_across_byte_boundaries() {
		let entry = 1 | (0x3ff << 2) | (0x155 << 12) | (0x2aa << 22) | (0xc << 32) | (0x5 << 36);
		let file = GimmickParameter::read(Cursor::new(parameters(1, &[0, entry]))).unwrap();

		let set = file.set(1);
		assert!(set.enabled());
		assert!(!set.animated());
		assert_eq!(set.rotation(), [0x3ff, 0x155, 0x2aa]);
		assert_eq!(set.unknown_a(), 0xc);
		assert_eq!(set.unknown_b(), 0x5);
	}

	/// Unlike .eqp, an omitted .gmp block is empty rather than a set of defaults.
	#[test]
	fn an_omitted_block_is_empty() {
		let file = GimmickParameter::read(Cursor::new(parameters(1, &[0, u64::MAX]))).unwrap();

		assert!(file.set(1).enabled());
		assert!(file.set(0).enabled());
		assert!(!file.set(200).enabled());
		assert_eq!(file.set(200).rotation(), [0, 0, 0]);
	}

	#[test]
	fn a_block_truncated_by_the_file_is_empty() {
		let mut bytes = parameters(1, &[0, u64::MAX]);
		bytes.truncate(40);
		let file = GimmickParameter::read(Cursor::new(bytes)).unwrap();

		assert!(file.set(1).enabled());
		assert!(!file.set(100).enabled());
	}
}
