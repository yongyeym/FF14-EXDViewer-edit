//! Structs and utilities for parsing .eid files.

use std::io::{Read, Seek, SeekFrom};

use binrw::{BinRead, BinResult, Endian, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

// ASCII "21", read little-endian.
const RADIAN_VERSION: i16 = 0x3132;

/// The points on a skeleton that weapons, effects and other objects are attached to.
#[binread]
#[br(little, magic = b"die\0")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct BindPoints {
	/// Tag naming the layout the entries are written in, as two ASCII digits read little-endian.
	version1: i16,

	version2: i16,

	#[br(temp)]
	count: u32,

	unknown: u32,

	#[br(parse_with = bind_points, args(version1, count))]
	#[getset(skip)]
	bind_points: Vec<BindPoint>,
}

impl BindPoints {
	/// Every bind point, in the order the file holds them.
	pub fn bind_points(&self) -> &[BindPoint] {
		&self.bind_points
	}

	/// Whether rotations are held in radians, which the layout tagged `0x3132` does. Files on the
	/// older layout hold them in degrees.
	pub fn radians(&self) -> bool {
		self.version1 == RADIAN_VERSION
	}
}

/// One point, named for the bone it hangs off.
#[binread]
#[br(little, import(radians: bool))]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct BindPoint {
	// The two layouts hold the same fields, the older one naming the bone after the transform
	// rather than before it, in a shorter buffer.
	#[br(temp, if(radians, [0; 32]))]
	leading_name: [u8; 32],

	id: i32,

	/// Position relative to the bone.
	position: [f32; 3],

	/// Rotation about each axis, in the unit [`BindPoints::radians`] names.
	rotation: [f32; 3],

	#[br(temp, if(!radians, [0; 12]))]
	trailing_name: [u8; 12],

	#[br(temp)]
	_padding: u32,

	#[br(calc = bone_name(if radians { &leading_name[..] } else { &trailing_name[..] }))]
	#[getset(skip)]
	name: String,
}

impl BindPoint {
	/// Name of the bone the point hangs off.
	pub fn name(&self) -> &str {
		&self.name
	}
}

fn bone_name(bytes: &[u8]) -> String {
	let end = bytes
		.iter()
		.position(|&byte| byte == 0)
		.unwrap_or(bytes.len());
	String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// An unrecognised version tag would otherwise fall to the older layout and read the entries at the
/// wrong stride, yielding garbage rather than failing, so the entries are required to fill the file.
fn bind_points<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(version1, count): (i16, u32),
) -> BinResult<Vec<BindPoint>> {
	let radians = version1 == RADIAN_VERSION;
	let stride = if radians { 64 } else { 44 };

	let start = reader.stream_position()?;
	let end = reader.seek(SeekFrom::End(0))?;
	reader.seek(SeekFrom::Start(start))?;

	if start + u64::from(count) * stride != end {
		return Err(binrw::Error::AssertFail {
			pos: start,
			message: format!(
				"{count} bind points of {stride} bytes do not fill the {} remaining",
				end - start
			),
		});
	}

	(0..count)
		.map(|_| BindPoint::read_options(reader, endian, (radians,)))
		.collect()
}

impl File for BindPoints {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::BindPoints;

	fn bind_points(version1: i16, points: &[(&str, i32, [f32; 3])]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"die\0");
		bytes.extend(version1.to_le_bytes());
		bytes.extend(0x3130i16.to_le_bytes());
		bytes.extend(u32::try_from(points.len()).unwrap().to_le_bytes());
		bytes.extend([0; 4]);

		for &(name, id, rotation) in points {
			if version1 == 0x3132 {
				bytes.extend(name_buffer(name, 32));
			}
			bytes.extend(id.to_le_bytes());
			for value in [1.0f32, 2.0, 3.0] {
				bytes.extend(value.to_le_bytes());
			}
			for value in rotation {
				bytes.extend(value.to_le_bytes());
			}
			if version1 != 0x3132 {
				bytes.extend(name_buffer(name, 12));
			}
			bytes.extend([0; 4]);
		}
		bytes
	}

	/// Filled past the terminator, which the read has to stop at rather than taking the buffer whole.
	fn name_buffer(name: &str, len: usize) -> Vec<u8> {
		let mut bytes = vec![0xcc; len];
		bytes[..name.len()].copy_from_slice(name.as_bytes());
		bytes[name.len()] = 0;
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			BindPoints::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn truncated() {
		let mut bytes = bind_points(0x3132, &[("n_root", 1, [0.0; 3]), ("n_hand", 2, [0.0; 3])]);
		bytes.truncate(bytes.len() - 4);
		assert!(matches!(
			BindPoints::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_the_radian_layout() {
		let file = BindPoints::read(Cursor::new(bind_points(
			0x3132,
			&[("n_root", 1, [0.5, 0.0, 0.0]), ("j_buki_l", 2, [0.0; 3])],
		)))
		.unwrap();

		assert!(file.radians());
		assert_eq!(file.version1(), 0x3132);
		assert_eq!(file.version2(), 0x3130);
		assert_eq!(file.bind_points().len(), 2);

		let point = &file.bind_points()[1];
		assert_eq!(point.name(), "j_buki_l");
		assert_eq!(point.id(), 2);
		assert_eq!(point.position(), [1.0, 2.0, 3.0]);
		assert_eq!(file.bind_points()[0].rotation(), [0.5, 0.0, 0.0]);
	}

	/// The older layout names the bone after the transform, and states rotations in degrees.
	#[test]
	fn reads_the_degree_layout() {
		let file = BindPoints::read(Cursor::new(bind_points(
			0x3130,
			&[("n_root", 1, [90.0, 0.0, 0.0]), ("j_buki_r", 2, [0.0; 3])],
		)))
		.unwrap();

		assert!(!file.radians());
		assert_eq!(file.bind_points().len(), 2);

		let point = &file.bind_points()[0];
		assert_eq!(point.name(), "n_root");
		assert_eq!(point.id(), 1);
		assert_eq!(point.position(), [1.0, 2.0, 3.0]);
		assert_eq!(point.rotation(), [90.0, 0.0, 0.0]);
		assert_eq!(file.bind_points()[1].name(), "j_buki_r");
	}

	/// A tag neither layout claims must not be read as the older one, whose shorter stride would
	/// take the second entry's name for the first entry's position.
	#[test]
	fn rejects_an_unrecognised_version() {
		let mut bytes = bind_points(0x3132, &[("n_root", 1, [0.0; 3]), ("n_hand", 2, [0.0; 3])]);
		bytes[4..6].copy_from_slice(&0x3133i16.to_le_bytes());
		assert!(matches!(
			BindPoints::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_a_file_with_no_points() {
		let file = BindPoints::read(Cursor::new(bind_points(0x3132, &[]))).unwrap();
		assert!(file.bind_points().is_empty());
	}
}
