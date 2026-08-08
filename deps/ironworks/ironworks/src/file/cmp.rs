//! Structs and utilities for parsing .cmp files.

use std::io::Cursor;

use binrw::{BinRead, binread};
use getset::{CopyGetters, Getters};

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

/// The colours character creation offers, and the range each of its sliders covers.
#[binread]
#[br(little)]
#[derive(Debug, Getters)]
#[get = "pub"]
pub struct CharacterMakeParameters {
	colors: ColorParameters,

	/// The same colours again. This is the block that matches what the game draws.
	interface_colors: ColorParameters,

	/// Indexed by `(tribe - 1) * 2` for a male character and one past that for a female one, taking
	/// the tribe as the row id of the `Tribe` sheet.
	// Sixteen clans by two genders. Held on the heap because the block is 160KB, which is more than
	// a wasm stack will carry through the moves reading it takes.
	#[br(count = 32)]
	races: Vec<ClanColors>,

	/// Indexed by `(tribe - 1) / 2` then `(tribe - 1) % 2`, which reaches only the first two slots of
	/// each group.
	scales: [[Scale; 10]; 8],
}

/// One palette of the colours a character can be given.
#[binread]
#[br(little)]
#[derive(Debug, Getters)]
#[get = "pub"]
pub struct ColorParameters {
	eyes: [Color; 256],
	hair_highlights: [Color; 256],

	#[getset(skip)]
	lips_dark: [Color; 128],

	#[getset(skip)]
	face_paint_dark: [Color; 128],

	features: [Color; 256],

	#[getset(skip)]
	lips_light: [Color; 128],

	#[getset(skip)]
	face_paint_light: [Color; 128],

	unused_eyes_a: [Color; 256],
	unused_eyes_b: [Color; 256],
	unused_eyes_c: [Color; 256],
	unused_features: [Color; 256],
}

impl ColorParameters {
	/// The lip colour an index names. The dark and light halves are indexed as one run of 256, the
	/// light half starting at 128.
	pub fn lips(&self, index: usize) -> Option<Color> {
		half(&self.lips_dark, &self.lips_light, index)
	}

	/// The face paint colour an index names, split into halves like [`lips`](Self::lips).
	pub fn face_paint(&self, index: usize) -> Option<Color> {
		half(&self.face_paint_dark, &self.face_paint_light, index)
	}
}

fn half(dark: &[Color; 128], light: &[Color; 128], index: usize) -> Option<Color> {
	match index.checked_sub(dark.len()) {
		Some(index) => light.get(index).copied(),
		None => dark.get(index).copied(),
	}
}

/// The colours available to one clan and gender.
#[binread]
#[br(little)]
#[derive(Debug, Getters)]
#[get = "pub"]
pub struct ClanColors {
	skin: [Color; 256],
	hair: [HairColor; 256],
	skin_interface: [Color; 256],
	hair_interface: [Color; 256],
}

/// A hair colour, and the sheen colour that goes unread beside it.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct HairColor {
	main: Color,
	unused_sheen: Color,
}

/// The range a clan's proportions can be adjusted over.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Scale {
	male_min_height: f32,
	male_max_height: f32,
	male_min_tail: f32,
	male_max_tail: f32,

	female_min_height: f32,
	female_max_height: f32,
	female_min_tail: f32,
	female_max_tail: f32,

	bust_min: [f32; 3],
	bust_max: [f32; 3],
}

/// A colour, stored in RGBA order.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, CopyGetters)]
#[get_copy = "pub"]
pub struct Color {
	red: u8,
	green: u8,
	blue: u8,
	alpha: u8,
}

/// Bytes the file takes, which is fixed.
const SIZE: usize = 0x2D980;

impl File for CharacterMakeParameters {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		// The file carries neither a magic nor a version, so the length is the only thing that can
		// catch a layout that is not the one below.
		if bytes.len() != SIZE {
			return Err(Error::Invalid(
				ErrorValue::Other("CMP".into()),
				format!("expected {SIZE} bytes, got {}", bytes.len()),
			));
		}
		Ok(<Self as BinRead>::read(&mut Cursor::new(bytes))?)
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{CharacterMakeParameters, Color, SIZE};

	/// A file with every colour set to its own offset within the block it sits in, so a field reading
	/// from the wrong place shows up as the wrong value rather than as zero.
	fn parameters() -> Vec<u8> {
		let colors = |count: usize| (0..count).flat_map(|index| color(index).to_vec());

		let block = || -> Vec<u8> {
			[256, 256, 128, 128, 256, 128, 128, 256, 256, 256, 256]
				.into_iter()
				.flat_map(colors)
				.collect()
		};

		let mut bytes = Vec::new();
		bytes.extend(block());
		bytes.extend(block());
		for _ in 0..32 {
			bytes.extend(colors(256));
			bytes.extend(colors(512));
			bytes.extend(colors(256));
			bytes.extend(colors(256));
		}
		for group in 0..8u32 {
			for slot in 0..10u32 {
				for field in 0..14 {
					let value = f32::from_bits(group * 100 + slot * 10 + field);
					bytes.extend(value.to_le_bytes());
				}
			}
		}

		assert_eq!(bytes.len(), SIZE);
		bytes
	}

	fn color(index: usize) -> [u8; 4] {
		let index = u16::try_from(index).unwrap().to_le_bytes();
		[index[0], index[1], 0, 0xFF]
	}

	fn at(index: usize) -> Color {
		let [red, green, blue, alpha] = color(index);
		Color {
			red,
			green,
			blue,
			alpha,
		}
	}

	#[test]
	fn empty() {
		assert!(matches!(
			CharacterMakeParameters::read(io::empty()),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn a_file_of_the_wrong_length_is_an_error() {
		let mut bytes = parameters();
		bytes.push(0);
		assert!(matches!(
			CharacterMakeParameters::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));

		let mut bytes = parameters();
		bytes.truncate(SIZE - 1);
		assert!(matches!(
			CharacterMakeParameters::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn reads_every_block_at_its_own_offset() {
		let file = CharacterMakeParameters::read(Cursor::new(parameters())).unwrap();

		assert_eq!(file.colors().eyes()[255], at(255));
		assert_eq!(file.colors().hair_highlights()[0], at(0));
		assert_eq!(file.colors().features()[128], at(128));
		assert_eq!(file.colors().unused_features()[255], at(255));
		assert_eq!(file.interface_colors().eyes()[12], at(12));

		let clan = &file.races()[31];
		assert_eq!(clan.skin()[100], at(100));
		assert_eq!(clan.hair()[100].main(), at(200));
		assert_eq!(clan.hair()[100].unused_sheen(), at(201));
		assert_eq!(clan.hair_interface()[255], at(255));

		let scale = file.scales()[7][1];
		assert_eq!(scale.male_min_height().to_bits(), 710);
		assert_eq!(scale.female_max_tail().to_bits(), 717);
		assert_eq!(scale.bust_min().map(f32::to_bits), [718, 719, 720]);
		assert_eq!(scale.bust_max().map(f32::to_bits), [721, 722, 723]);
	}

	/// Lip and face paint indices run across the dark half into the light one.
	#[test]
	fn lips_and_face_paint_span_both_halves() {
		let file = CharacterMakeParameters::read(Cursor::new(parameters())).unwrap();
		let colors = file.colors();

		assert_eq!(colors.lips(0), Some(at(0)));
		assert_eq!(colors.lips(127), Some(at(127)));
		assert_eq!(colors.lips(128), Some(at(0)));
		assert_eq!(colors.lips(255), Some(at(127)));
		assert_eq!(colors.lips(256), None);

		assert_eq!(colors.face_paint(128), Some(at(0)));
		assert_eq!(colors.face_paint(256), None);
	}
}
