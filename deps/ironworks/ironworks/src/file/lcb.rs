//! Structs and utilities for parsing .lcb files.

use std::io::SeekFrom;

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// Bytes of a group's header the entry offset is measured past.
const FIELDS: i64 = 12;

/// The boxes a zone clips its lights against.
///
/// A zone's scene names the file it uses.
#[binread]
#[br(little, magic = b"LCB1")]
#[derive(Debug)]
pub struct ClipBoxes {
	#[br(temp, pad_before = 4)]
	count: u32,

	#[br(count = count)]
	groups: Vec<Group>,
}

impl ClipBoxes {
	/// The groups this file carries.
	pub fn groups(&self) -> &[Group] {
		&self.groups
	}
}

impl File for ClipBoxes {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// A run of entries. The size the group declares is that of its own header, and the entries sit at
/// an offset measured from the end of the magic and that size rather than straight after it.
#[binread]
#[br(little, magic = b"LCC1")]
#[derive(Debug, CopyGetters)]
pub struct Group {
	/// Zero in every file the game ships.
	#[br(pad_before = 4)]
	#[get_copy = "pub"]
	version: i32,

	#[br(temp)]
	entries_offset: i32,

	#[br(temp)]
	count: u32,

	#[br(seek_before = SeekFrom::Current(i64::from(entries_offset) - FIELDS), count = count)]
	entries: Vec<Entry>,
}

impl Group {
	/// The entries this group carries.
	pub fn entries(&self) -> &[Entry] {
		&self.entries
	}
}

/// The box one light is clipped against.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Entry {
	/// Key of an instance in one of the zone's layers.
	instance: u32,

	/// Reaches the light inside that instance, an index per level of shared group it sits under,
	/// filled from the front and zero the rest of the way.
	members: [u8; 4],

	min: [f32; 3],
	max: [f32; 3],
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::ClipBoxes;

	/// A file whose groups place their entries `entries_offset` past their own fields, with the
	/// gap filled so a reader landing anywhere else reads the filler as entries.
	fn boxes(entries_offset: u32, groups: &[&[u32]]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"LCB1");
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(u32::try_from(groups.len()).unwrap().to_le_bytes());

		for entries in groups {
			bytes.extend(b"LCC1");
			bytes.extend(20u32.to_le_bytes());
			bytes.extend(0u32.to_le_bytes());
			bytes.extend(entries_offset.to_le_bytes());
			bytes.extend(u32::try_from(entries.len()).unwrap().to_le_bytes());
			bytes.extend(vec![0xCC; entries_offset as usize - 12]);
			for &instance in *entries {
				bytes.extend(instance.to_le_bytes());
				bytes.extend([2, 1, 0, 0]);
				bytes.extend((0..3).flat_map(|axis| (-(axis as f32)).to_le_bytes()));
				bytes.extend((0..3).flat_map(|axis| (axis as f32).to_le_bytes()));
			}
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			ClipBoxes::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_every_group() {
		let file = ClipBoxes::read(Cursor::new(boxes(12, &[&[1, 2], &[], &[3]]))).unwrap();

		let groups = file.groups();
		assert_eq!(groups.len(), 3);
		assert_eq!(groups[0].version(), 0);
		assert_eq!(
			groups[0]
				.entries()
				.iter()
				.map(|entry| entry.instance())
				.collect::<Vec<_>>(),
			[1, 2]
		);
		assert!(groups[1].entries().is_empty());

		let entry = groups[2].entries()[0];
		assert_eq!(entry.members(), [2, 1, 0, 0]);
		assert_eq!(entry.min(), [-0., -1., -2.]);
		assert_eq!(entry.max(), [0., 1., 2.]);
	}

	/// Entries sit where the group's own offset puts them, which every shipping file writes as 12
	/// and so cannot tell apart from the size the group declares.
	#[test]
	fn entries_start_at_the_offset_the_group_gives() {
		let file = ClipBoxes::read(Cursor::new(boxes(20, &[&[7]]))).unwrap();
		assert_eq!(file.groups()[0].entries()[0].instance(), 7);
	}
}
