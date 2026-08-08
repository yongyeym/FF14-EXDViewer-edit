//! Structs and utilities for parsing .uwb files.

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// The underwater companion to a zone's clip boxes.
///
/// A zone's scene says whether it has one.
#[binread]
#[br(little, magic = b"UWB1")]
#[derive(Debug)]
pub struct Underwater {
	#[br(temp, pad_before = 4)]
	count: u32,

	#[br(count = count)]
	groups: Vec<Group>,
}

impl Underwater {
	/// The groups this file carries.
	pub fn groups(&self) -> &[Group] {
		&self.groups
	}
}

impl File for Underwater {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// One set of underwater settings. The whole group is its own header, so the size it declares is
/// the eighty-eight bytes below.
#[binread]
#[br(little, magic = b"UWC1")]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Group {
	/// Zero in every file the game ships.
	#[br(pad_before = 4)]
	version: i32,

	/// Height the water surface sits at, which the depth of a point is measured down from.
	water_surface_y: f32,

	/// Where the blend from [`fog_shallow`](Self::fog_shallow) to [`fog_deep`](Self::fog_deep)
	/// starts, and how far it runs.
	depth_transition_start: f32,
	depth_transition_range: f32,

	/// Fog just under the surface.
	fog_shallow: Fog,

	/// Fog past the depth transition.
	fog_deep: Fog,

	/// Where caustics start fading with distance, and how far that fade runs.
	caustics_distance_fade_start: f32,
	caustics_distance_fade_range: f32,

	/// The two scales the caustic pattern is sampled at.
	caustics_uv_size: [f32; 2],

	caustics_scroll_speed: f32,
	caustics_intensity: f32,

	sun_size: f32,
	sun_fade_start: f32,

	/// Scales everything lit underwater.
	lighting_multiplier: f32,

	/// The client tests this for zero and does nothing else with it.
	unknown: u32,
}

/// How far light carries through the water at one depth.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Fog {
	vertical_fade_upper: f32,
	vertical_fade_lower: f32,
	vertical_attenuation_strength: f32,
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::Underwater;

	fn underwater(groups: &[f32]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"UWB1");
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(u32::try_from(groups.len()).unwrap().to_le_bytes());

		for &seed in groups {
			bytes.extend(b"UWC1");
			bytes.extend(88u32.to_le_bytes());
			bytes.extend(0u32.to_le_bytes());
			bytes.extend((0..18).flat_map(|index| (seed + index as f32).to_le_bytes()));
			bytes.extend(7u32.to_le_bytes());
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			Underwater::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_every_group() {
		let file = Underwater::read(Cursor::new(underwater(&[0., 100.]))).unwrap();

		let groups = file.groups();
		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].water_surface_y(), 0.);
		assert_eq!(groups[1].water_surface_y(), 100.);
	}

	/// Every value after the version, in the order the format writes them.
	#[test]
	fn reads_each_setting_at_its_own_offset() {
		let file = Underwater::read(Cursor::new(underwater(&[0.]))).unwrap();
		let group = file.groups()[0];

		assert_eq!(group.version(), 0);
		assert_eq!(group.depth_transition_start(), 1.);
		assert_eq!(group.depth_transition_range(), 2.);
		assert_eq!(group.fog_shallow().vertical_fade_upper(), 3.);
		assert_eq!(group.fog_deep().vertical_attenuation_strength(), 8.);
		assert_eq!(group.caustics_distance_fade_start(), 9.);
		assert_eq!(group.caustics_uv_size(), [11., 12.]);
		assert_eq!(group.sun_size(), 15.);
		assert_eq!(group.lighting_multiplier(), 17.);
		assert_eq!(group.unknown(), 7);
	}
}
