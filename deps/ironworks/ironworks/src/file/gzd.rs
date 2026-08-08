//! Structs and utilities for parsing .gzd files.

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// Versions from this one on carry a third auto layer.
const VERSION_THIRD_LAYER: u32 = 0x0200_0500;

/// Versions from this one on carry a trailing value per auto layer.
const VERSION_LAYER_VALUES: u32 = 0x0200_0600;

fn string<const N: usize>(bytes: &[u8; N]) -> String {
	let end = bytes.iter().position(|&byte| byte == 0).unwrap_or(N);
	String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Every grass grid a zone is covered by, and the models and textures they draw with.
///
/// A zone ships one of these as `grass_zone_data.gzd` in its `grass` directory, beside the `.ggd`
/// files it names. It is the only index of those: their names are a grid coordinate rather than
/// anything the rest of the zone spells out.
#[binread]
#[br(little, magic = b"dzg\0")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct GrassZone {
	/// `0x02000600` in almost every file the game ships.
	version: u32,

	#[br(temp)]
	high_count: u16,
	#[br(temp)]
	medium_count: u16,
	#[br(temp)]
	low_count: u16,

	/// Model slots a grid carries past its auto layers, which is 32 in every file the game ships
	/// and bounds [`model_paths`](Self::model_paths).
	model_slot_capacity: u8,

	#[br(temp)]
	model_path_count: u8,

	#[br(
		count = if version >= VERSION_THIRD_LAYER { 3 } else { 2 },
		map = |raw: Vec<[u8; 0x20]>| raw.iter().map(string).collect(),
	)]
	#[getset(skip)]
	color_map: Vec<String>,

	/// Per auto layer, and 1.0 in every file that carries it. Only `0x02000600` and later do.
	#[br(if(version >= VERSION_LAYER_VALUES))]
	auto_layer_values: Option<[f32; 3]>,

	#[br(
		count = model_path_count,
		map = |raw: Vec<[u8; 0x100]>| raw.iter().map(string).collect(),
	)]
	#[getset(skip)]
	model_paths: Vec<String>,

	#[br(count = high_count)]
	#[getset(skip)]
	high: Vec<Grid>,

	#[br(count = medium_count)]
	#[getset(skip)]
	medium: Vec<Grid>,

	#[br(count = low_count)]
	#[getset(skip)]
	low: Vec<Grid>,
}

impl GrassZone {
	/// The grids covering the zone at one level of detail.
	pub fn grids(&self, detail: Detail) -> &[Grid] {
		match detail {
			Detail::High => &self.high,
			Detail::Medium => &self.medium,
			Detail::Low => &self.low,
		}
	}

	/// Per auto layer, the base name of the colour map it samples, at `<zone directory>/<name>.tex`.
	/// A layer with no map is named by an empty string. Files before `0x02000500` carry two layers
	/// rather than three.
	pub fn color_map(&self) -> &[String] {
		&self.color_map
	}

	/// The models the grids place, in the order their count slots past the auto layers name them.
	pub fn model_paths(&self) -> &[String] {
		&self.model_paths
	}
}

/// Level of detail one grid holds, which is also the suffix of its file name.
#[binread]
#[br(repr = u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
	/// Drawn nearest the camera, as `h`.
	High = 0,

	/// As `m`.
	Medium = 1,

	/// Drawn furthest from the camera, as `l`.
	Low = 2,
}

/// One grass grid of a zone, and where the file holding it sits in the world.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Grid {
	/// The point the grid's placements are measured from, which is the world origin its own file
	/// declares.
	center: [f32; 3],

	/// Radius of the sphere the grid is sorted and culled by, about [`center`](Self::center).
	radius: f32,

	/// Level of detail the grid holds, which always matches the array it is stored in.
	detail: Detail,

	/// Cell the grid covers, counted in cells of 32 world units, ordered X, Y and Z. The file holds
	/// them the other way around.
	#[br(map = |raw: [u8; 3]| [raw[2], raw[1], raw[0]])]
	cell: [u8; 3],
}

impl Grid {
	/// Name of the grid's file, in the directory the zone file itself sits in.
	pub fn file(&self) -> String {
		let suffix = match self.detail {
			Detail::High => 'h',
			Detail::Medium => 'm',
			Detail::Low => 'l',
		};
		let [x, y, z] = self.cell;
		format!("{x:03}_{y:03}_{z:03}_{suffix}.ggd")
	}
}

impl File for GrassZone {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Detail, GrassZone, VERSION_LAYER_VALUES, VERSION_THIRD_LAYER};

	fn padded(value: &str, size: usize) -> Vec<u8> {
		let mut bytes = vec![0u8; size];
		bytes[..value.len()].copy_from_slice(value.as_bytes());
		bytes
	}

	/// A zone, as its colour maps, its model paths, and the cells covered at each level of detail.
	fn zone(
		version: u32,
		maps: &[&str],
		models: &[&str],
		grids: [&[([u8; 3], [f32; 3])]; 3],
	) -> Vec<u8> {
		let header = match version {
			VERSION_LAYER_VALUES.. => 0x7C,
			VERSION_THIRD_LAYER.. => 0x70,
			_ => 0x50,
		};

		let mut bytes = Vec::from(*b"dzg\0");
		bytes.extend(version.to_le_bytes());
		for tier in grids {
			bytes.extend(u16::try_from(tier.len()).unwrap().to_le_bytes());
		}
		bytes.push(32);
		bytes.push(u8::try_from(models.len()).unwrap());
		for map in maps {
			bytes.extend(padded(map, 0x20));
		}
		if version >= VERSION_LAYER_VALUES {
			for value in [1.0f32; 3] {
				bytes.extend(value.to_le_bytes());
			}
		}
		assert_eq!(bytes.len(), header);

		for model in models {
			bytes.extend(padded(model, 0x100));
		}
		for (detail, tier) in grids.iter().enumerate() {
			for (cell, center) in *tier {
				for axis in center {
					bytes.extend(axis.to_le_bytes());
				}
				bytes.extend(28.293f32.to_le_bytes());
				bytes.extend([u8::try_from(detail).unwrap(), cell[2], cell[1], cell[0]]);
			}
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			GrassZone::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_a_grid_at_each_level_of_detail() {
		let file = GrassZone::read(Cursor::new(zone(
			0x0200_0600,
			&["_grass1", "", "_grass3"],
			&["bg/ffxiv/fst_f1/bgparts/f1_grass.mdl"],
			[
				&[
					([1, 2, 3], [32.0, 64.0, 96.0]),
					([2, 2, 3], [64.0, 64.0, 96.0]),
				],
				&[([4, 5, 6], [128.0, 160.0, 192.0])],
				&[],
			],
		)))
		.unwrap();

		assert_eq!(file.version(), 0x0200_0600);
		assert_eq!(file.model_slot_capacity(), 32);
		assert_eq!(file.auto_layer_values(), Some([1.0; 3]));
		assert_eq!(file.color_map(), ["_grass1", "", "_grass3"]);
		assert_eq!(file.model_paths(), ["bg/ffxiv/fst_f1/bgparts/f1_grass.mdl"]);

		assert_eq!(file.grids(Detail::High).len(), 2);
		assert!(file.grids(Detail::Low).is_empty());

		let grid = file.grids(Detail::Medium)[0];
		assert_eq!(grid.center(), [128.0, 160.0, 192.0]);
		assert_eq!(grid.cell(), [4, 5, 6]);
		assert_eq!(grid.detail(), Detail::Medium);
	}

	/// A grid names its file by its cell, zero padded, and by its level of detail.
	#[test]
	fn names_its_grid_files() {
		let file = GrassZone::read(Cursor::new(zone(
			0x0200_0600,
			&[""; 3],
			&[],
			[
				&[([9, 5, 24], [0.0; 3])],
				&[([0, 0, 0], [0.0; 3])],
				&[([255, 12, 100], [0.0; 3])],
			],
		)))
		.unwrap();

		assert_eq!(file.grids(Detail::High)[0].file(), "009_005_024_h.ggd");
		assert_eq!(file.grids(Detail::Medium)[0].file(), "000_000_000_m.ggd");
		assert_eq!(file.grids(Detail::Low)[0].file(), "255_012_100_l.ggd");
	}

	/// The oldest files carry two auto layers and no trailing values, putting their header at 0x50.
	#[test]
	fn reads_the_two_layer_version() {
		let file = GrassZone::read(Cursor::new(zone(
			0x0200_0000,
			&["_grass1", "_grass4"],
			&[],
			[&[], &[], &[]],
		)))
		.unwrap();

		assert_eq!(file.color_map(), ["_grass1", "_grass4"]);
		assert_eq!(file.auto_layer_values(), None);
		assert!(file.grids(Detail::High).is_empty());
	}

	#[test]
	fn truncated_grids() {
		let mut bytes = zone(
			0x0200_0600,
			&[""; 3],
			&[],
			[&[([0, 0, 0], [0.0; 3]); 2], &[], &[]],
		);
		bytes.truncate(bytes.len() - 1);
		assert!(matches!(
			GrassZone::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn a_level_of_detail_the_format_does_not_name() {
		let mut bytes = zone(
			0x0200_0600,
			&[""; 3],
			&[],
			[&[([0, 0, 0], [0.0; 3])], &[], &[]],
		);
		let detail = bytes.len() - 4;
		bytes[detail] = 3;
		assert!(matches!(
			GrassZone::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
