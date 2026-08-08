//! Structs and utilities for parsing .est files.

use std::fmt::Debug;

use binrw::{BinRead, binread};

use crate::{FileStream, error::Result};

use super::File;

/// The skeleton to use for a given set on a given gender and race.
///
/// What a set ID names is per-file: an equipment set for `extra_met` and `extra_top`, a character
/// creation option for `faceSkeletonTemplate` and `hairSkeletonTemplate`.
#[binread]
#[br(little)]
pub struct ExtraSkeletonTemplate {
	#[br(temp)]
	count: u32,

	// Set ID at +0 and gender/race at +2 read little-endian as one ascending u32, so a pair can be
	// looked up by binary search over the keys.
	#[br(count = count)]
	keys: Vec<u32>,

	#[br(count = count)]
	skeletons: Vec<u16>,
}

impl ExtraSkeletonTemplate {
	/// Get the skeleton ID for a set on a gender and race, if the file specifies one. Gender and
	/// race are the character model code, as in the 0101 of `chara/human/c0101`, and include values
	/// used only by NPCs.
	pub fn skeleton(&self, gender_race: u16, set: u16) -> Option<u16> {
		let key = (u32::from(gender_race) << 16) | u32::from(set);
		let index = self.keys.binary_search(&key).ok()?;
		self.skeletons.get(index).copied()
	}

	/// Iterate over every entry, as (gender and race, set ID, skeleton ID).
	pub fn entries(&self) -> impl Iterator<Item = (u16, u16, u16)> {
		self.keys
			.iter()
			.zip(&self.skeletons)
			.map(|(&key, &skeleton)| ((key >> 16) as u16, key as u16, skeleton))
	}
}

impl File for ExtraSkeletonTemplate {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

impl Debug for ExtraSkeletonTemplate {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("ExtraSkeletonTemplate")
			.field("entries.len", &self.keys.len())
			.finish()
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::ExtraSkeletonTemplate;

	fn template(entries: &[(u16, u16, u16)]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(u32::try_from(entries.len()).unwrap().to_le_bytes());
		for &(gender_race, set, _) in entries {
			bytes.extend(set.to_le_bytes());
			bytes.extend(gender_race.to_le_bytes());
		}
		for &(_, _, skeleton) in entries {
			bytes.extend(skeleton.to_le_bytes());
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			ExtraSkeletonTemplate::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn truncated() {
		let mut bytes = template(&[(101, 2, 3), (101, 5, 6)]);
		bytes.truncate(bytes.len() - 2);
		assert!(matches!(
			ExtraSkeletonTemplate::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn finds_a_set_within_a_gender_and_race() {
		let file = ExtraSkeletonTemplate::read(Cursor::new(template(&[
			(101, 2, 11),
			(101, 5, 22),
			(9204, 1, 33),
		])))
		.unwrap();

		assert_eq!(file.skeleton(101, 5), Some(22));
		assert_eq!(file.skeleton(9204, 1), Some(33));
		assert_eq!(file.skeleton(101, 1), None);
		assert_eq!(file.skeleton(201, 5), None);
		assert_eq!(file.entries().count(), 3);
	}

	/// `faceSkeletonTemplate.est` carries one duplicated key, so neither the lookup nor iteration
	/// may assume uniqueness.
	#[test]
	fn tolerates_a_duplicate_key() {
		let file = ExtraSkeletonTemplate::read(Cursor::new(template(&[
			(101, 2, 11),
			(101, 2, 22),
			(101, 5, 33),
		])))
		.unwrap();

		assert!(matches!(file.skeleton(101, 2), Some(11 | 22)));
		assert_eq!(file.skeleton(101, 5), Some(33));
		assert_eq!(file.entries().count(), 3);
	}
}
