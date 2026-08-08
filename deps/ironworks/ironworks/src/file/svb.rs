//! Structs and utilities for parsing .svb files.

use std::io::SeekFrom;

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// Bytes of a group's header the entry offset is measured past.
const FIELDS: i64 = 12;

/// How the sky reads through a zone's instances.
///
/// A zone's scene names the file it uses.
#[binread]
#[br(little, magic = b"SVB1")]
#[derive(Debug)]
pub struct SkyVisibility {
	#[br(temp, pad_before = 4)]
	count: u32,

	#[br(count = count)]
	groups: Vec<Group>,
}

impl SkyVisibility {
	/// The groups this file carries.
	pub fn groups(&self) -> &[Group] {
		&self.groups
	}
}

impl File for SkyVisibility {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// A run of entries. The size the group declares is that of its own header, and the entries sit at
/// an offset measured from the end of the magic and that size rather than straight after it.
#[binread]
#[br(little, magic = b"SVC1")]
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

/// What one instance does to the sky.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Entry {
	/// Key of an instance in one of the zone's layers.
	instance: u32,

	/// Reaches the part of that instance the entry applies to, an index per level of shared group
	/// it sits under, filled from the front and zero the rest of the way.
	members: [u8; 4],

	/// How much of the sky reaches it, over `0.0..=1.0`.
	visibility: f32,
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::SkyVisibility;

	/// A file whose groups place their entries `entries_offset` past their own fields, with the
	/// gap filled so a reader landing anywhere else reads the filler as entries.
	fn visibility(entries_offset: u32, groups: &[&[u32]]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"SVB1");
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(u32::try_from(groups.len()).unwrap().to_le_bytes());

		for entries in groups {
			bytes.extend(b"SVC1");
			bytes.extend(20u32.to_le_bytes());
			bytes.extend(0u32.to_le_bytes());
			bytes.extend(entries_offset.to_le_bytes());
			bytes.extend(u32::try_from(entries.len()).unwrap().to_le_bytes());
			bytes.extend(vec![0xCC; entries_offset as usize - 12]);
			for &instance in *entries {
				bytes.extend(instance.to_le_bytes());
				bytes.extend([2, 1, 0, 0]);
				bytes.extend(0.5f32.to_le_bytes());
			}
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			SkyVisibility::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_every_group() {
		let file = SkyVisibility::read(Cursor::new(visibility(12, &[&[1, 2], &[], &[3]]))).unwrap();

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
		assert_eq!(entry.visibility(), 0.5);
	}

	/// Entries sit where the group's own offset puts them, which every shipping file writes as 12
	/// and so cannot tell apart from the size the group declares.
	#[test]
	fn entries_start_at_the_offset_the_group_gives() {
		let file = SkyVisibility::read(Cursor::new(visibility(20, &[&[7]]))).unwrap();
		assert_eq!(file.groups()[0].entries()[0].instance(), 7);
	}
}
