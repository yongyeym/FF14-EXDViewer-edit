//! Structs and utilities for parsing .pap files.

use std::io::{Read, Seek, SeekFrom};

use binrw::{BinRead, BinResult, Endian, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// The animations one skeleton can play: a Havok animation container, and the timeline each
/// animation is driven by.
#[binread]
#[br(little, magic = b"pap ")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct AnimationPack {
	/// `0x00020001` in every file the game ships.
	version: u32,

	#[br(temp)]
	animation_count: u16,

	/// Model these animations are built for, as in the `0101` of `c0101`.
	model_id: u16,

	model_type: ModelType,

	variant: u8,

	// The skipped offset names the animation table, which always follows the 26-byte header.
	#[br(temp, pad_before = 4)]
	havok_offset: u32,

	#[br(temp)]
	timeline_offset: u32,

	#[br(count = animation_count)]
	#[getset(skip)]
	animations: Vec<Animation>,

	#[br(parse_with = region, args(havok_offset.into(), timeline_offset.into()))]
	#[getset(skip)]
	havok: Vec<u8>,

	#[br(parse_with = timelines, args(timeline_offset.into(), animation_count.into()))]
	#[getset(skip)]
	timelines: Vec<Vec<u8>>,
}

impl AnimationPack {
	/// Every animation of the pack.
	pub fn animations(&self) -> &[Animation] {
		&self.animations
	}

	/// The animations themselves, in Havok binary tagfile format. Files carrying no animation of
	/// their own hold eight bytes or fewer here.
	pub fn havok(&self) -> &[u8] {
		&self.havok
	}

	/// The timeline driving each animation, in `.tmb` format, ordered as
	/// [`animations`](Self::animations).
	pub fn timelines(&self) -> &[Vec<u8>] {
		&self.timelines
	}
}

impl File for AnimationPack {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// The kind of skeleton a pack's animations are built for.
#[allow(missing_docs)]
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
	#[br(magic = 0u8)]
	Human,

	#[br(magic = 1u8)]
	Monster,

	#[br(magic = 2u8)]
	DemiHuman,

	#[br(magic = 3u8)]
	Weapon,

	Unknown(u8),
}

/// One animation of a pack.
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Animation {
	#[br(map = |name: [u8; 32]| {
		let end = name.iter().position(|byte| *byte == 0).unwrap_or(name.len());
		String::from_utf8_lossy(&name[..end]).into_owned()
	})]
	#[getset(skip)]
	name: String,

	/// Kind of animation, which is `22` in the material animations of `material.pap`.
	animation_type: u16,

	/// Index of the motion this animation plays, within the Havok animation container.
	havok_index: i16,

	#[br(map = |face: i32| face != 0)]
	face: bool,
}

impl Animation {
	/// Name of the animation.
	pub fn name(&self) -> &str {
		&self.name
	}
}

/// Reads `start..end`, which the file must be long enough to hold.
fn region<R: Read + Seek>(
	reader: &mut R,
	_endian: Endian,
	(start, end): (u64, u64),
) -> BinResult<Vec<u8>> {
	let length = reader.seek(SeekFrom::End(0))?;
	if start > end || end > length {
		return Err(binrw::Error::AssertFail {
			pos: start,
			message: format!("region {start}..{end} lies outside the {length} byte file"),
		});
	}

	reader.seek(SeekFrom::Start(start))?;
	let mut bytes = vec![0; usize::try_from(end - start).unwrap()];
	reader.read_exact(&mut bytes)?;
	Ok(bytes)
}

/// Reads the timeline blocks, walking each one's own size to find the next.
fn timelines<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(start, count): (u64, usize),
) -> BinResult<Vec<Vec<u8>>> {
	let mut at = start;
	let mut timelines = Vec::with_capacity(count);

	for index in 0..count {
		reader.seek(SeekFrom::Start(at))?;
		let magic = <[u8; 4]>::read_options(reader, endian, ())?;
		if &magic != b"TMLB" {
			return Err(binrw::Error::BadMagic {
				pos: at,
				found: Box::new(magic),
			});
		}

		let size = u32::read_options(reader, endian, ())?;
		timelines.push(region(reader, endian, (at, at + u64::from(size)))?);
		at += u64::from(size);

		// Blocks sit at multiples of four from the first, which is not itself four-aligned as the
		// header and animation table are 26 + 40n bytes. The last block carries no padding.
		if index + 1 < count {
			at = start + (at - start).next_multiple_of(4);
		}
	}

	Ok(timelines)
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{AnimationPack, ModelType};

	fn timeline(body: &[u8]) -> Vec<u8> {
		let mut bytes = Vec::from(*b"TMLB");
		bytes.extend(u32::try_from(body.len() + 8).unwrap().to_le_bytes());
		bytes.extend(body);
		bytes
	}

	fn pack(model_type: u8, names: &[&str], havok: &[u8], timelines: &[Vec<u8>]) -> Vec<u8> {
		let havok_offset = 26 + 40 * names.len();
		let timeline_offset = havok_offset + havok.len();

		let mut bytes = Vec::new();
		bytes.extend(b"pap ");
		bytes.extend(0x0002_0001u32.to_le_bytes());
		bytes.extend(u16::try_from(names.len()).unwrap().to_le_bytes());
		bytes.extend(101u16.to_le_bytes());
		bytes.extend([model_type, 3]);
		bytes.extend(26u32.to_le_bytes());
		bytes.extend(u32::try_from(havok_offset).unwrap().to_le_bytes());
		bytes.extend(u32::try_from(timeline_offset).unwrap().to_le_bytes());

		for (index, name) in names.iter().enumerate() {
			let mut field = [0; 32];
			field[..name.len()].copy_from_slice(name.as_bytes());
			bytes.extend(field);
			bytes.extend(17u16.to_le_bytes());
			bytes.extend(i16::try_from(index).unwrap().to_le_bytes());
			bytes.extend(i32::from(index == 0).to_le_bytes());
		}

		bytes.extend(havok);

		for (index, timeline) in timelines.iter().enumerate() {
			bytes.extend(timeline);
			if index + 1 < timelines.len() {
				let padded = (bytes.len() - timeline_offset).next_multiple_of(4);
				// Nothing promises the padding is zeroed.
				bytes.resize(timeline_offset + padded, 0xCC);
			}
		}

		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			AnimationPack::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	/// Twelve bytes of Havok leave the first block at 118, which is not four-aligned, so a reader
	/// aligning the blocks to the file rather than to the first of them looks in the wrong place.
	#[test]
	fn reads_animations_and_payloads() {
		let timelines = [timeline(&[1; 9]), timeline(&[2; 5])];
		let file = AnimationPack::read(Cursor::new(pack(
			0,
			&["cbbm_id0", "cbbm_id1"],
			&[9; 12],
			&timelines,
		)))
		.unwrap();

		assert_eq!(file.version(), 0x0002_0001);
		assert_eq!(file.model_id(), 101);
		assert_eq!(file.model_type(), ModelType::Human);
		assert_eq!(file.variant(), 3);

		let animations = file.animations();
		assert_eq!(animations.len(), 2);
		assert_eq!(animations[0].name(), "cbbm_id0");
		assert_eq!(animations[0].animation_type(), 17);
		assert!(animations[0].face());
		assert_eq!(animations[1].havok_index(), 1);
		assert!(!animations[1].face());

		assert_eq!(file.havok(), [9; 12]);
		assert_eq!(file.timelines(), timelines);
	}

	#[test]
	fn no_animations() {
		let file = AnimationPack::read(Cursor::new(pack(1, &[], &[9; 8], &[]))).unwrap();
		assert_eq!(file.model_type(), ModelType::Monster);
		assert!(file.animations().is_empty());
		assert!(file.timelines().is_empty());
		assert_eq!(file.havok(), [9; 8]);
	}

	#[test]
	fn empty_havok() {
		let timelines = [timeline(&[1; 4])];
		let file =
			AnimationPack::read(Cursor::new(pack(2, &["cbbm_id0"], &[0; 8], &timelines))).unwrap();
		assert_eq!(file.model_type(), ModelType::DemiHuman);
		assert_eq!(file.havok(), [0; 8]);
		assert_eq!(file.timelines(), timelines);
	}

	#[test]
	fn unknown_model_type() {
		let file = AnimationPack::read(Cursor::new(pack(9, &[], &[0; 8], &[]))).unwrap();
		assert_eq!(file.model_type(), ModelType::Unknown(9));
	}

	/// Each block's own size says where the next one starts, so a block carrying the magic in its
	/// body splits no differently to one that does not.
	#[test]
	fn timeline_bodies_are_not_scanned_for_the_magic() {
		let mut body = vec![0; 15];
		body[3..7].copy_from_slice(b"TMLB");
		let timelines = [timeline(&body), timeline(&[2; 5])];
		let file = AnimationPack::read(Cursor::new(pack(
			0,
			&["cbbm_id0", "cbbm_id1"],
			&[9; 12],
			&timelines,
		)))
		.unwrap();
		assert_eq!(file.timelines(), timelines);
	}

	#[test]
	fn truncated_timeline() {
		let mut bytes = pack(
			0,
			&["cbbm_id0", "cbbm_id1"],
			&[9; 12],
			&[timeline(&[1; 9]), timeline(&[2; 5])],
		);
		bytes.truncate(bytes.len() - 2);
		assert!(matches!(
			AnimationPack::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
