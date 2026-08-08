//! Structs and utilities for parsing .imc files.

use binrw::{BinRead, binread, helpers::until_eof};

use crate::{FileStream, error::Result};

use super::File;

/// The material, decal, VFX and attributes to draw a model set with, per variant.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct ImageChange {
	variant_count: u16,
	part_mask: u16,

	// The header is never trusted: entries are read to the end of the file and looked up by index,
	// so a count disagreeing with the file cannot fail the read.
	#[br(parse_with = until_eof)]
	entries: Vec<Entry>,
}

impl ImageChange {
	/// The number of variants beyond the default.
	pub fn variant_count(&self) -> u16 {
		self.variant_count
	}

	/// Mask of the parts this file carries. Equipment and accessories use every part, weapons and
	/// monsters only the first.
	pub fn part_mask(&self) -> u16 {
		self.part_mask
	}

	/// Every entry, in variant then part order.
	pub fn entries(&self) -> &[Entry] {
		&self.entries
	}

	/// Get the entry for a part and variant, variant 0 being the default. A part is the position of
	/// an equipment slot within its set - head or ears 0, body or neck 1, hands or wrists 2, legs or
	/// right ring 3, feet or left ring 4 - and 0 for anything with a single part.
	pub fn entry(&self, part: u8, variant: u16) -> Option<&Entry> {
		let mask = 1u16.checked_shl(part.into())?;
		if self.part_mask & mask == 0 {
			return None;
		}

		let parts = usize::try_from(self.part_mask.count_ones()).unwrap();
		self.entries
			.get(usize::from(variant) * parts + usize::from(part))
	}
}

impl File for ImageChange {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// The appearance of one part of one variant.
#[binread]
#[br(map = Self)]
#[derive(Debug)]
pub struct Entry(bitfield::Entry);

impl Entry {
	/// The material to draw with.
	pub fn material_id(&self) -> u8 {
		self.0.material_id()
	}

	/// The decal to draw, or 0 for none.
	pub fn decal_id(&self) -> u8 {
		self.0.decal_id()
	}

	/// Mask of the model attributes enabled for this variant, `a` in the lowest bit.
	pub fn attribute_mask(&self) -> u16 {
		self.0.attribute_mask()
	}

	/// The footstep and equipment sound to use, or 0 for none.
	pub fn sound_id(&self) -> u8 {
		self.0.sound_id()
	}

	/// The VFX to draw, or 0 for none.
	pub fn vfx_id(&self) -> u8 {
		self.0.vfx_id()
	}

	/// The material animation to run, or 0 for none.
	pub fn material_animation_id(&self) -> u8 {
		self.0.material_animation_id()
	}
}

// `#[bitfield]` parenthesises each field's type where `#[derive(BinRead)]` then reads it back, so
// the lint fires on generated spans and points at the visibility keyword.
#[allow(dead_code, unused_parens)]
mod bitfield {
	use binrw::BinRead;
	use modular_bitfield::prelude::*;

	#[bitfield]
	#[derive(BinRead, Debug)]
	#[br(map = Self::from_bytes)]
	pub struct Entry {
		pub material_id: u8,
		pub decal_id: u8,
		pub attribute_mask: B10,
		pub sound_id: B6,
		pub vfx_id: u8,
		pub material_animation_id: B4,
		#[skip]
		_reserved: B4,
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::ImageChange;

	fn image_change(variant_count: u16, part_mask: u16, entries: &[[u8; 6]]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(variant_count.to_le_bytes());
		bytes.extend(part_mask.to_le_bytes());
		bytes.extend(entries.iter().flatten());
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			ImageChange::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn unpacks_an_entry() {
		let file = ImageChange::read(Cursor::new(image_change(
			0,
			1,
			&[[1, 2, 0x34, 0x12, 5, 0x36]],
		)))
		.unwrap();

		let entry = file.entry(0, 0).unwrap();
		assert_eq!(entry.material_id(), 1);
		assert_eq!(entry.decal_id(), 2);
		assert_eq!(entry.attribute_mask(), 0x234);
		assert_eq!(entry.sound_id(), 4);
		assert_eq!(entry.vfx_id(), 5);
		assert_eq!(entry.material_animation_id(), 6);
	}

	#[test]
	fn indexes_variant_before_part() {
		let entries = (0..10)
			.map(|index| [index, 0, 0, 0, 0, 0])
			.collect::<Vec<_>>();
		let file = ImageChange::read(Cursor::new(image_change(1, 31, &entries))).unwrap();

		assert_eq!(file.entry(3, 0).unwrap().material_id(), 3);
		assert_eq!(file.entry(3, 1).unwrap().material_id(), 8);
	}

	#[test]
	fn a_part_outside_the_mask_has_no_entry() {
		let file =
			ImageChange::read(Cursor::new(image_change(0, 1, &[[1, 0, 0, 0, 0, 0]]))).unwrap();

		assert!(file.entry(0, 0).is_some());
		assert!(file.entry(1, 0).is_none());
		assert!(file.entry(200, 0).is_none());
	}

	/// The header can claim more variants than the file carries, and a trailing partial entry can
	/// leave the read short of the end.
	#[test]
	fn a_truncated_file_reads_the_entries_it_has() {
		let mut bytes = image_change(4, 1, &[[1, 0, 0, 0, 0, 0], [2, 0, 0, 0, 0, 0]]);
		bytes.extend([3, 0, 0]);
		let file = ImageChange::read(Cursor::new(bytes)).unwrap();

		assert_eq!(file.variant_count(), 4);
		assert_eq!(file.entries().len(), 2);
		assert_eq!(file.entry(0, 1).unwrap().material_id(), 2);
		assert!(file.entry(0, 2).is_none());
	}
}
