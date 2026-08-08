//! Structs and utilities for parsing .tera files.

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// The terrain of a zone: which plates it is tiled from, and where each of them sits.
///
/// The file carries no geometry of its own. Each plate is a `.mdl` sitting beside the terrain file,
/// named by its position in [`plates`](Self::plates).
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Terrain {
	/// `0x01000003`; earlier versions lay the header out differently.
	version: u32,

	#[br(temp)]
	plate_count: u32,

	/// Side length of one plate, in world units.
	plate_size: u32,

	/// Distance past which the terrain is not drawn, which is zero in most files.
	clip_distance: f32,

	/// How far a plate's textures blend into its neighbours, over `0.0..=1.0`.
	edge_bias: f32,

	/// Mask of the texture slots the plate materials sample with the alternate mip LOD bias, the
	/// colour slot in the lowest bit, then normal and specular. No other bit is ever set.
	///
	/// Both Physis and Lumina read this offset as padding, but it carries a value.
	#[br(pad_after = 28)]
	sampler_bias: u32,

	#[br(count = plate_count)]
	#[getset(skip)]
	plates: Vec<Plate>,
}

impl Terrain {
	/// Every plate, ordered by the index its model file is named for.
	pub fn plates(&self) -> &[Plate] {
		&self.plates
	}

	/// Name of a plate's model, in the directory the terrain file itself sits in.
	pub fn plate_file(index: usize) -> String {
		format!("{index:04}.mdl")
	}

	/// Centre of a plate in world units, on the X and Z axes. Each plate model carries its own
	/// vertical positions.
	pub fn plate_position(&self, plate: Plate) -> (f32, f32) {
		let size = self.plate_size as f32;
		(
			size * (f32::from(plate.x) + 0.5),
			size * (f32::from(plate.y) + 0.5),
		)
	}
}

/// The cell one plate covers, counted in plates from the world origin.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Plate {
	x: i16,
	y: i16,
}

impl File for Terrain {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::Terrain;

	fn terrain(plate_size: u32, sampler_bias: u32, plates: &[(i16, i16)]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(0x01000003u32.to_le_bytes());
		bytes.extend(u32::try_from(plates.len()).unwrap().to_le_bytes());
		bytes.extend(plate_size.to_le_bytes());
		bytes.extend(0.0f32.to_le_bytes());
		bytes.extend(1.0f32.to_le_bytes());
		bytes.extend(sampler_bias.to_le_bytes());
		bytes.extend([0; 28]);
		for &(x, y) in plates {
			bytes.extend(x.to_le_bytes());
			bytes.extend(y.to_le_bytes());
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			Terrain::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn truncated_plates() {
		let mut bytes = terrain(128, 0, &[(0, 0), (1, 0)]);
		bytes.truncate(bytes.len() - 2);
		assert!(matches!(
			Terrain::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn plates_are_placed_at_their_centres() {
		let file = Terrain::read(Cursor::new(terrain(
			128,
			0,
			&[(-1, -1), (0, -1), (-1, 0), (0, 0)],
		)))
		.unwrap();

		assert_eq!(file.plate_size(), 128);
		assert_eq!(file.plates().len(), 4);
		assert_eq!(Terrain::plate_file(3), "0003.mdl");

		let plates = file.plates();
		assert_eq!(file.plate_position(plates[0]), (-64.0, -64.0));
		assert_eq!(file.plate_position(plates[1]), (64.0, -64.0));
		assert_eq!(file.plate_position(plates[3]), (64.0, 64.0));
	}

	/// The four bytes at 0x14 carry a value rather than the padding both other readers take them for.
	#[test]
	fn sampler_bias_is_read_rather_than_skipped() {
		let file = Terrain::read(Cursor::new(terrain(32, 5, &[(2, 3)]))).unwrap();
		assert_eq!(file.sampler_bias(), 5);
		assert_eq!(file.edge_bias(), 1.0);
		assert_eq!(file.plates()[0].x(), 2);
		assert_eq!(file.plate_position(file.plates()[0]), (80.0, 112.0));
	}
}
