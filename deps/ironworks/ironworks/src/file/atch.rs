//! Structs and utilities for parsing .atch files.

use std::{
	fmt::{self, Debug},
	io::SeekFrom,
	str,
};

use binrw::{BinRead, NullString, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// Where a character's weapons and tools sit on their skeleton.
#[binread]
#[br(little)]
pub struct AttachPoints {
	#[br(temp)]
	point_count: u16,

	state_count: u16,

	#[br(count = point_count)]
	tags: Vec<Tag>,

	// A fixed 256 bits however many points the file holds, some of which set bits past the last of
	// them.
	accessories: [u8; 32],

	#[br(count = usize::from(point_count) * usize::from(state_count))]
	states: Vec<State>,
}

impl AttachPoints {
	/// The number of states every point carries.
	pub fn state_count(&self) -> u16 {
		self.state_count
	}

	/// The type tag of every point, in the order the file holds them.
	pub fn tags(&self) -> &[Tag] {
		&self.tags
	}

	/// The index of the point a tag names.
	pub fn point(&self, tag: &str) -> Option<usize> {
		self.tags.iter().position(|it| it.as_str() == Some(tag))
	}

	/// The accessory flag at a point's index.
	pub fn accessory(&self, point: usize) -> bool {
		self.accessories
			.get(point / 8)
			.is_some_and(|byte| (byte >> (point % 8)) & 1 == 1)
	}

	/// The states of a point, in the order the file holds them.
	pub fn states(&self, point: usize) -> Option<&[State]> {
		if point >= self.tags.len() {
			return None;
		}
		let count = usize::from(self.state_count);
		let start = point * count;
		self.states.get(start..start + count)
	}
}

/// The three characters naming what attaches at a point, which the file holds reversed.
#[binread]
#[br(little, map = Self::from_bytes)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Tag([u8; 4]);

impl Tag {
	fn from_bytes(mut bytes: [u8; 4]) -> Self {
		let end = terminator(&bytes);
		bytes[..end].reverse();
		Self(bytes)
	}

	/// The tag as written, or `None` if it is not text.
	pub fn as_str(&self) -> Option<&str> {
		str::from_utf8(&self.0[..terminator(&self.0)]).ok()
	}
}

fn terminator(bytes: &[u8; 4]) -> usize {
	bytes
		.iter()
		.position(|&byte| byte == 0)
		.unwrap_or(bytes.len())
}

/// One placement of a point.
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct State {
	#[br(temp)]
	bone_offset: u32,

	// The offset is absolute, and reading the name where it points bounds-checks it for free.
	#[br(
		seek_before = SeekFrom::Start(bone_offset.into()),
		restore_position,
		map = |bone: NullString| bone.to_string(),
	)]
	#[getset(skip)]
	bone: String,

	scale: f32,

	/// Position relative to the bone.
	offset: [f32; 3],

	/// Rotation about each axis, in radians.
	rotation: [f32; 3],
}

impl State {
	/// Name of the bone the point hangs off in this state.
	pub fn bone(&self) -> &str {
		&self.bone
	}
}

impl File for AttachPoints {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

impl Debug for AttachPoints {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("AttachPoints")
			.field("state_count", &self.state_count)
			.field("tags", &self.tags)
			.finish()
	}
}

impl Debug for Tag {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.as_str() {
			Some(tag) => write!(f, "{tag:?}"),
			None => write!(f, "{:?}", self.0),
		}
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::AttachPoints;

	fn attach_points(points: &[(&str, bool, &[&str])]) -> Vec<u8> {
		let state_count = points.first().map_or(0, |(_, _, bones)| bones.len());

		let mut bytes = Vec::new();
		bytes.extend(u16::try_from(points.len()).unwrap().to_le_bytes());
		bytes.extend(u16::try_from(state_count).unwrap().to_le_bytes());

		for &(tag, ..) in points {
			let mut field = [0; 4];
			field[..tag.len()].copy_from_slice(tag.as_bytes());
			field[..tag.len()].reverse();
			bytes.extend(field);
		}

		let mut accessories = [0u8; 32];
		for (index, &(_, accessory, _)) in points.iter().enumerate() {
			if accessory {
				accessories[index / 8] |= 1 << (index % 8);
			}
		}
		bytes.extend(accessories);

		let pool = bytes.len() + 32 * points.len() * state_count;
		let mut strings = Vec::new();
		for &(_, _, bones) in points {
			for &bone in bones {
				bytes.extend(u32::try_from(pool + strings.len()).unwrap().to_le_bytes());
				strings.extend(bone.as_bytes());
				strings.push(0);

				for value in [0.5f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
					bytes.extend(value.to_le_bytes());
				}
			}
		}
		bytes.extend(strings);
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			AttachPoints::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	/// Losing the end of the string pool leaves a bone name unterminated, which the offsets pointing
	/// into it must not read past.
	#[test]
	fn truncated() {
		let mut bytes = attach_points(&[("swd", false, &["n_buki_r"])]);
		bytes.truncate(bytes.len() - 1);
		assert!(matches!(
			AttachPoints::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	/// A bone offset is absolute, and nothing else in the file bounds it.
	#[test]
	fn rejects_a_bone_offset_past_the_end() {
		let mut bytes = attach_points(&[("swd", false, &["n_buki_r"])]);
		bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
		assert!(matches!(
			AttachPoints::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	/// A tag is stored reversed, and as a fixed four bytes rather than up to its terminator.
	#[test]
	fn reverses_the_type_tag() {
		let file = AttachPoints::read(Cursor::new(attach_points(&[("2ax", false, &["n_buki_r"])])))
			.unwrap();

		assert_eq!(file.tags().len(), 1);
		assert_eq!(file.tags()[0].as_str(), Some("2ax"));
		assert_eq!(file.point("2ax"), Some(0));
		assert_eq!(file.point("xa2"), None);
	}

	/// The flag block is indexed by bit, so a point past the eighth reads from a later byte.
	#[test]
	fn accessory_flags_are_bit_indexed() {
		let tags = [
			"2ax", "2bk", "2bw", "2ff", "2gb", "2gl", "2gn", "2km", "2kt", "2kz",
		];
		let points = tags
			.iter()
			.enumerate()
			.map(|(index, &tag)| (tag, matches!(index, 1 | 9), &["n_buki_r"][..]))
			.collect::<Vec<_>>();

		let file = AttachPoints::read(Cursor::new(attach_points(&points))).unwrap();

		assert!(!file.accessory(0));
		assert!(file.accessory(1));
		assert!(!file.accessory(8));
		assert!(file.accessory(9));
	}

	#[test]
	fn states_run_in_blocks_per_point() {
		let file = AttachPoints::read(Cursor::new(attach_points(&[
			("swd", false, &["n_buki_r", "j_buki_kosi_r"]),
			("sld", true, &["n_buki_l", "j_buki_sebo_l"]),
		])))
		.unwrap();

		assert_eq!(file.state_count(), 2);
		assert_eq!(
			file.states(0)
				.unwrap()
				.iter()
				.map(|state| state.bone())
				.collect::<Vec<_>>(),
			["n_buki_r", "j_buki_kosi_r"]
		);
		assert_eq!(file.states(1).unwrap()[1].bone(), "j_buki_sebo_l");
		assert!(file.states(2).is_none());

		let state = &file.states(1).unwrap()[0];
		assert_eq!(state.scale(), 0.5);
		assert_eq!(state.offset(), [1.0, 2.0, 3.0]);
		assert_eq!(state.rotation(), [4.0, 5.0, 6.0]);
	}

	/// `chara/xls/attachoffset/d1011.atch` declares no states, while carrying records an
	/// unidentified older layout would read.
	#[test]
	fn reads_a_file_with_no_states() {
		let file = AttachPoints::read(Cursor::new(attach_points(&[("swd", false, &[])]))).unwrap();

		assert_eq!(file.state_count(), 0);
		assert!(file.states(0).unwrap().is_empty());
		assert!(file.states(1).is_none());
	}
}
