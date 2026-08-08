//! Structs and utilities for parsing .sklb files.

use std::io::{Read, Seek, SeekFrom};

use binrw::helpers::until_eof;
use binrw::{BinRead, BinResult, Endian, binread};
use getset::{CopyGetters, Getters};

use crate::{FileStream, error::Result};

use super::{animation, file::File};

pub use animation::AnimationLayer;

/// Skeleton data and related mappings.
#[binread]
#[br(little, magic = b"blks")]
#[derive(Debug, Getters, CopyGetters)]
pub struct SkeletonBinary {
	// File header.
	/// Version of this skelton file. This is a XIV-specific version tag, and does
	/// not directly correlate with the version of the embedded tagfile.
	#[get_copy = "pub"]
	version: Version,

	#[br(args(version))]
	header: Header,

	// Animation Layers.
	///
	#[br(
		seek_before = SeekFrom::Start(header.layer_offset().into()),
		parse_with = animation::layers,
	)]
	#[get = "pub"]
	animation_layers: Vec<AnimationLayer>,

	/// Skeleton data, in Havok binary tagfile format.
	#[br(
		seek_before = SeekFrom::Start(header.skeleton_offset().into()),
		parse_with = until_eof,
	)]
	#[get = "pub"]
	skeleton: Vec<u8>,
}

impl SkeletonBinary {
	/// ID of the character associated with this skeleton.
	pub fn character_id(&self) -> u32 {
		match &self.header {
			Header::V1(header) => header.character_id,
			Header::V2(header) => header.character_id,
		}
	}

	///
	pub fn mapper_character_id(&self) -> [u32; 4] {
		match &self.header {
			Header::V1(header) => header.mapper_character_id,
			Header::V2(header) => header.mapper_character_id,
		}
	}

	///
	pub fn connect_bones(&self) -> Vec<i16> {
		match (&self.header, self.version) {
			(Header::V1(header), _) => header.connect_bones.to_vec(),
			(Header::V2(header), Version::V1301) => header.connect_bones.to_vec(),
			(Header::V2(header), _) => vec![header.connect_bone_index],
		}
	}

	///
	pub fn lod_sample_bone_count(&self) -> Option<[i16; 3]> {
		match &self.header {
			Header::V1(header) => Some(header.lod_sample_bone_count),
			Header::V2(_) => None,
		}
	}
}

impl File for SkeletonBinary {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// XIV skeleton file version.
#[allow(missing_docs)]
#[binread]
#[br(little)]
#[derive(Clone, Copy, Debug)]
pub enum Version {
	#[br(magic = b"0011")]
	V1100,

	#[br(magic = b"0111")]
	V1110,

	#[br(magic = b"0021")]
	V1200,

	#[br(magic = b"0031")]
	V1300,

	#[br(magic = b"1031")]
	V1301,
}

#[derive(Debug)]
enum Header {
	V1(HeaderV1),
	V2(HeaderV2),
}

impl Header {
	fn layer_offset(&self) -> u32 {
		match self {
			Self::V1(header) => header.layer_offset.into(),
			Self::V2(header) => header.layer_offset,
		}
	}

	fn skeleton_offset(&self) -> u32 {
		match self {
			Self::V1(header) => header.skeleton_offset.into(),
			Self::V2(header) => header.skeleton_offset,
		}
	}
}

impl BinRead for Header {
	type Args<'a> = (Version,);

	fn read_options<R: Read + Seek>(
		reader: &mut R,
		_options: Endian,
		(version,): Self::Args<'_>,
	) -> BinResult<Self> {
		match version {
			Version::V1100 | Version::V1110 | Version::V1200 => {
				Ok(Self::V1(HeaderV1::read(reader)?))
			}
			Version::V1300 | Version::V1301 => Ok(Self::V2(HeaderV2::read(reader)?)),
		}
	}
}

#[binread]
#[br(little)]
#[derive(Debug)]
struct HeaderV1 {
	layer_offset: u16,
	skeleton_offset: u16,
	character_id: u32,
	mapper_character_id: [u32; 4],
	lod_sample_bone_count: [i16; 3],
	connect_bones: [i16; 4],
}

#[binread]
#[br(little)]
#[derive(Debug)]
struct HeaderV2 {
	layer_offset: u32,
	skeleton_offset: u32,
	connect_bone_index: i16,
	#[br(pad_before = 2)]
	character_id: u32,
	mapper_character_id: [u32; 4],
	connect_bones: [i16; 4],
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use crate::file::File;

	use super::SkeletonBinary;

	/// A new-style skeleton of the given version, carrying no animation layers and a one-byte
	/// skeleton block.
	fn skeleton(version: &[u8; 4], connect_bone_index: i16, connect_bones: [i16; 4]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"blks");
		bytes.extend(version);
		bytes.extend(48u32.to_le_bytes());
		bytes.extend(54u32.to_le_bytes());
		bytes.extend(connect_bone_index.to_le_bytes());
		bytes.extend([0; 2]);
		bytes.extend(1301u32.to_le_bytes());
		bytes.extend([0xFF; 16]);
		bytes.extend(connect_bones.iter().flat_map(|bone| bone.to_le_bytes()));
		bytes.extend(b"hpla");
		bytes.extend(0u16.to_le_bytes());
		bytes.push(0);
		bytes
	}

	#[test]
	fn reads_a_bone_list_from_the_newest_header() {
		let bytes = skeleton(b"1031", 0, [11, 59, 60, -1]);
		let file = SkeletonBinary::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.character_id(), 1301);
		assert_eq!(file.connect_bones(), [11, 59, 60, -1]);
	}

	/// The version before it names one bone, and leaves the list's bytes empty.
	#[test]
	fn reads_a_single_bone_from_the_prior_header() {
		let bytes = skeleton(b"0031", 46, [0; 4]);
		let file = SkeletonBinary::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.connect_bones(), [46]);
	}
}
