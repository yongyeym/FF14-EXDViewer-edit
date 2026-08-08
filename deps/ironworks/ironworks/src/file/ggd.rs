//! Structs and utilities for parsing .ggd files.

use std::io::{Read, Seek, SeekFrom};

use binrw::{BinRead, BinResult, Endian, binread};
use getset::CopyGetters;
use half::f16;

use crate::{FileStream, error::Result};

use super::File;

/// Versions from this one on carry sixteen trailing bytes of per-profile alignment tuning.
const VERSION_ALIGNMENT: u32 = 0x0200_0500;

fn half(bits: u16) -> f32 {
	f16::from_bits(bits).to_f32()
}

/// Where the grass of one terrain plate grows, at one level of detail.
///
/// The zone's `grass_zone_data.gzd` names every one of these, as `<x>_<y>_<z>_h`, `_m` or `_l` in
/// the zone's `grass` directory, for the cell it covers and the level of detail it holds. The grid
/// divides that cell into chunks, each holding the placements that fall inside it.
#[binread]
#[br(little, magic = b" dgg")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct GrassGrid {
	/// `0x02000800` in almost every file the game ships, `0x02000402` in the rest, which hold
	/// sixteen fewer bytes of header.
	version: u32,

	#[br(temp)]
	chunk_count: u16,

	/// `0x0555` in every file the game ships.
	unknown_a: u16,

	// Slots past chunk_count hold stale offsets, so only the declared ones are followed.
	#[br(temp)]
	chunk_offsets: [u32; 8],

	/// Per auto layer, the range a placement's lateral offset is drawn from. Read from half
	/// precision.
	#[br(map = |raw: [u16; 3]| raw.map(half))]
	lateral_offset_min: [f32; 3],
	#[br(map = |raw: [u16; 3]| raw.map(half))]
	lateral_offset_max: [f32; 3],

	/// Per auto layer, the range a placement's yaw is drawn from. Layers 0 and 1 are in radians;
	/// layer 2 carries the same magnitudes as degrees.
	#[br(map = |raw: [u16; 3]| raw.map(half))]
	yaw_min: [f32; 3],
	#[br(map = |raw: [u16; 3]| raw.map(half))]
	yaw_max: [f32; 3],

	/// The point every placement's position is measured from.
	world_origin: [f32; 3],

	/// Eight profiles of blend towards the terrain normal, as a fraction of 255. Only
	/// `0x02000500` and later carry these.
	#[br(if(version >= VERSION_ALIGNMENT))]
	alignment_bend_weight: Option<[u8; 8]>,

	/// Paired with [`alignment_bend_weight`](Self::alignment_bend_weight), and zero in almost
	/// every file the game ships.
	#[br(if(version >= VERSION_ALIGNMENT))]
	alignment_length_gain: Option<[u8; 8]>,

	#[br(parse_with = chunks, args(chunk_count.into(), chunk_offsets))]
	#[getset(skip)]
	chunks: Vec<Chunk>,
}

impl GrassGrid {
	/// Every chunk of the grid, in the order the header names them.
	pub fn chunks(&self) -> &[Chunk] {
		&self.chunks
	}
}

/// One placement of grass, positioned relative to its grid's
/// [`world_origin`](GrassGrid::world_origin).
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Placement {
	#[br(map = |raw: [u16; 3]| raw.map(half))]
	position: [f32; 3],

	/// A unit quaternion.
	#[br(map = |raw: [u16; 4]| raw.map(half))]
	rotation: [f32; 4],

	#[br(map = half)]
	scale_y: f32,
	#[br(map = half)]
	scale_xz: f32,

	/// Where in the wind cycle this placement starts, over `0.0..=1.0`.
	#[br(map = half)]
	wind_phase: f32,
	#[br(map = half)]
	wetness: f32,

	/// A variation selector, five bits wide. It outruns the header's eight
	/// [`alignment_bend_weight`](GrassGrid::alignment_bend_weight) profiles, so it does not index
	/// them directly.
	profile: u8,

	unknown_a: u8,
	unknown_b: u8,
	unknown_c: u8,
}

/// One chunk of a grid, and the placements that fall inside it.
#[binread]
#[br(little, magic = b"dgs\0")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Chunk {
	/// Bounds of the chunk, in world units.
	min: [f32; 3],
	max: [f32; 3],

	#[getset(skip)]
	counts: [u16; Self::COUNTS],

	#[br(count = counts.iter().copied().map(usize::from).sum::<usize>())]
	#[getset(skip)]
	placements: Vec<Placement>,
}

impl Chunk {
	/// Count slots a chunk carries. The leading [`AUTO_LAYERS`](Self::AUTO_LAYERS) are procedural
	/// grass layers, the rest name the models the zone's `.gzd` lists, and the last is unused.
	pub const COUNTS: usize = 36;

	/// Leading count slots that hold procedural grass rather than models.
	pub const AUTO_LAYERS: usize = 3;

	/// How many placements each layer and model group contributes, in the order they are stored.
	pub fn counts(&self) -> &[u16; Self::COUNTS] {
		&self.counts
	}

	/// Every placement in the chunk, grouped as [`counts`](Self::counts) describes.
	pub fn placements(&self) -> &[Placement] {
		&self.placements
	}
}

impl File for GrassGrid {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// Reads the chunks the header declares, each at the offset naming it.
fn chunks<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(count, offsets): (usize, [u32; 8]),
) -> BinResult<Vec<Chunk>> {
	if count > offsets.len() {
		return Err(binrw::Error::AssertFail {
			pos: 8,
			message: format!(
				"{count} chunks declared, but only {} are named",
				offsets.len()
			),
		});
	}

	offsets[..count]
		.iter()
		.map(|offset| {
			reader.seek(SeekFrom::Start((*offset).into()))?;
			Chunk::read_options(reader, endian, ())
		})
		.collect()
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Chunk, GrassGrid, VERSION_ALIGNMENT, f16};

	/// A placement, from values chosen to survive the round trip through half precision.
	fn placement(position: [f32; 3], profile: u8) -> Vec<u8> {
		let mut bytes = Vec::new();
		for value in position
			.iter()
			.copied()
			.chain([0.0, 0.0, 0.0, 1.0])
			.chain([1.0, 0.5, 0.25, 0.125])
		{
			bytes.extend(f16::from_f32(value).to_bits().to_le_bytes());
		}
		bytes.extend([profile, 0x11, 0x22, 0x33]);
		bytes
	}

	/// A chunk, as its bounds and the placements each count slot contributes.
	fn chunk(min: [f32; 3], groups: &[(usize, Vec<Vec<u8>>)]) -> Vec<u8> {
		let mut bytes = Vec::from(*b"dgs\0");
		for axis in min {
			bytes.extend(axis.to_le_bytes());
		}
		for axis in min {
			bytes.extend((axis + 32.0).to_le_bytes());
		}

		let mut counts = [0u16; Chunk::COUNTS];
		for (at, placements) in groups {
			counts[*at] = u16::try_from(placements.len()).unwrap();
		}
		for count in counts {
			bytes.extend(count.to_le_bytes());
		}

		for (_, placements) in groups {
			for placement in placements {
				bytes.extend(placement);
			}
		}
		bytes
	}

	/// A grid holding the given chunks, laid out one after another past the header.
	fn grid(version: u32, stale: &[u32], chunks: &[Vec<u8>]) -> Vec<u8> {
		let header = if version >= VERSION_ALIGNMENT {
			0x60
		} else {
			0x50
		};

		let mut offsets = [0u32; 8];
		let mut at = u32::try_from(header).unwrap();
		for (slot, chunk) in offsets.iter_mut().zip(chunks) {
			*slot = at;
			at += u32::try_from(chunk.len()).unwrap();
		}
		for (slot, value) in offsets.iter_mut().skip(chunks.len()).zip(stale) {
			*slot = *value;
		}

		let mut bytes = Vec::from(*b" dgg");
		bytes.extend(version.to_le_bytes());
		bytes.extend(u16::try_from(chunks.len()).unwrap().to_le_bytes());
		bytes.extend(0x0555u16.to_le_bytes());
		for offset in offsets {
			bytes.extend(offset.to_le_bytes());
		}
		// 0.25 and 0.5 in the first two lateral slots, then ten more half-precision zeroes.
		for raw in [0x3400u16, 0x3800] {
			bytes.extend(raw.to_le_bytes());
		}
		bytes.extend([0; 20]);
		for axis in [100.0f32, 200.0, 300.0] {
			bytes.extend(axis.to_le_bytes());
		}
		if version >= VERSION_ALIGNMENT {
			bytes.extend([95u8; 8]);
			bytes.extend([0, 51, 0, 0, 0, 0, 0, 0]);
		}
		assert_eq!(bytes.len(), header);

		for chunk in chunks {
			bytes.extend(chunk);
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			GrassGrid::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_a_chunk_at_each_declared_offset() {
		let file = GrassGrid::read(Cursor::new(grid(
			0x0200_0800,
			&[],
			&[
				chunk([0.0, 0.0, 0.0], &[(0, vec![placement([1.0, 2.0, 3.0], 0)])]),
				chunk(
					[64.0, 0.0, 0.0],
					&[
						(0, vec![placement([4.0, 5.0, 6.0], 1)]),
						(3, vec![placement([7.0, 8.0, 9.0], 2)]),
					],
				),
			],
		)))
		.unwrap();

		assert_eq!(file.version(), 0x0200_0800);
		assert_eq!(file.lateral_offset_min(), [0.25, 0.5, 0.0]);
		assert_eq!(file.world_origin(), [100.0, 200.0, 300.0]);
		assert_eq!(file.alignment_bend_weight(), Some([95; 8]));
		assert_eq!(
			file.alignment_length_gain(),
			Some([0, 51, 0, 0, 0, 0, 0, 0])
		);

		assert_eq!(file.chunks().len(), 2);
		let chunk = &file.chunks()[1];
		assert_eq!(chunk.min(), [64.0, 0.0, 0.0]);
		assert_eq!(chunk.max(), [96.0, 32.0, 32.0]);
		assert_eq!(chunk.counts()[..4], [1, 0, 0, 1]);
		assert_eq!(chunk.placements().len(), 2);
	}

	/// Placements decode to the fields the format names, not to an opaque run of bytes.
	#[test]
	fn decodes_a_placement() {
		let file = GrassGrid::read(Cursor::new(grid(
			0x0200_0800,
			&[],
			&[chunk(
				[0.0; 3],
				&[(0, vec![placement([1.5, -2.5, 3.5], 6)])],
			)],
		)))
		.unwrap();

		let placement = file.chunks()[0].placements()[0];
		assert_eq!(placement.position(), [1.5, -2.5, 3.5]);
		assert_eq!(placement.rotation(), [0.0, 0.0, 0.0, 1.0]);
		assert_eq!(placement.scale_y(), 1.0);
		assert_eq!(placement.scale_xz(), 0.5);
		assert_eq!(placement.wind_phase(), 0.25);
		assert_eq!(placement.wetness(), 0.125);
		assert_eq!(placement.profile(), 6);
		assert_eq!(placement.unknown_a(), 0x11);
	}

	/// `0x02000402` files hold sixteen fewer bytes of header, putting their first chunk at 0x50.
	#[test]
	fn reads_the_short_header_version() {
		let file = GrassGrid::read(Cursor::new(grid(
			0x0200_0402,
			&[],
			&[chunk([5.0, 6.0, 7.0], &[(0, vec![placement([0.0; 3], 0)])])],
		)))
		.unwrap();

		assert_eq!(file.lateral_offset_min(), [0.25, 0.5, 0.0]);
		assert_eq!(file.world_origin(), [100.0, 200.0, 300.0]);
		assert_eq!(file.alignment_bend_weight(), None);
		assert_eq!(file.alignment_length_gain(), None);
		assert_eq!(file.chunks()[0].min(), [5.0, 6.0, 7.0]);
		assert_eq!(file.chunks()[0].placements().len(), 1);
	}

	/// Placements are counted from every slot, not only the first few that files usually fill.
	#[test]
	fn sums_the_whole_count_array() {
		let file = GrassGrid::read(Cursor::new(grid(
			0x0200_0800,
			&[],
			&[chunk(
				[0.0; 3],
				&[
					(0, vec![placement([0.0; 3], 0)]),
					(5, vec![placement([1.0; 3], 0); 2]),
					(30, vec![placement([3.0; 3], 0); 4]),
				],
			)],
		)))
		.unwrap();

		let chunk = &file.chunks()[0];
		assert_eq!(chunk.counts()[30], 4);
		assert_eq!(chunk.placements().len(), 7);
	}

	/// The run ends where the counts say, not at the end of the file.
	#[test]
	fn ignores_bytes_past_the_last_placement() {
		let mut bytes = grid(
			0x0200_0800,
			&[],
			&[chunk([0.0; 3], &[(0, vec![placement([0.0; 3], 0)])])],
		);
		bytes.extend([0xcd; 40]);

		let file = GrassGrid::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.chunks()[0].placements().len(), 1);
	}

	/// Offsets past the declared count are stale, and pointing them anywhere must not matter.
	#[test]
	fn ignores_stale_offsets() {
		let file = GrassGrid::read(Cursor::new(grid(
			0x0200_0800,
			&[0xdead, 4, 0],
			&[chunk([0.0; 3], &[(0, vec![placement([0.0; 3], 0)])])],
		)))
		.unwrap();
		assert_eq!(file.chunks().len(), 1);
	}

	#[test]
	fn truncated_placements() {
		let mut bytes = grid(
			0x0200_0800,
			&[],
			&[chunk([0.0; 3], &[(0, vec![placement([0.0; 3], 0); 2])])],
		);
		bytes.truncate(bytes.len() - 1);
		assert!(matches!(
			GrassGrid::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn more_chunks_than_the_header_can_name() {
		let mut bytes = grid(
			0x0200_0800,
			&[],
			&[chunk([0.0; 3], &[(0, vec![placement([0.0; 3], 0)])])],
		);
		bytes[8..10].copy_from_slice(&9u16.to_le_bytes());
		assert!(matches!(
			GrassGrid::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
