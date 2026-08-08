//! Structs and utilities for parsing .amb files.

use std::io::Cursor;

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

/// Tracks an [`EnvLocation`] carries.
pub const TRACK_COUNT: usize = 32;

/// The two formats the `.amb` extension is used for, told apart by the byte at 0x07.
#[derive(Debug)]
pub enum Ambient {
	/// The light of one environment location.
	EnvLocation(EnvLocation),

	/// The light of every sky.
	SkyLight(SkyLight),
}

impl File for Ambient {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;

		let kind = Header::read(&mut Cursor::new(&bytes))?.kind;
		let mut cursor = Cursor::new(&bytes);
		Ok(match kind {
			0 => Self::EnvLocation(<EnvLocation as BinRead>::read(&mut cursor)?),
			1 => Self::SkyLight(<SkyLight as BinRead>::read(&mut cursor)?),
			other => {
				return Err(Error::Invalid(
					ErrorValue::Other("AMB".into()),
					format!("unknown kind {other}"),
				));
			}
		})
	}
}

#[binread]
#[br(little, magic = b"AMB\0")]
struct Header {
	#[br(pad_before = 3)]
	kind: u8,
}

/// The light one environment location casts, as tracks the day runs along.
#[binread]
#[br(little, magic = b"AMB\0")]
#[derive(Debug, CopyGetters)]
pub struct EnvLocation {
	/// `1` in every file the game ships.
	#[get_copy = "pub"]
	version: u16,

	#[br(temp, assert(kind == [0, 0]))]
	kind: [u8; 2],

	/// How much of the sky reaches the location, over the same basis as a [`Harmonics`] channel.
	#[get_copy = "pub"]
	sky_visibility: [f32; 9],

	counts: [u32; TRACK_COUNT],

	#[br(count = counts.iter().map(|&count| count as usize).sum::<usize>())]
	keyframes: Vec<Keyframe>,
}

impl EnvLocation {
	/// The keyframes of one of the [`TRACK_COUNT`] tracks, ascending by time. What the index
	/// selects has not been identified.
	pub fn track(&self, index: usize) -> Option<&[Keyframe]> {
		let count = *self.counts.get(index)? as usize;
		let start = self.counts[..index]
			.iter()
			.map(|&count| count as usize)
			.sum::<usize>();
		self.keyframes.get(start..start + count)
	}
}

/// The light each sky casts.
#[binread]
#[br(little, magic = b"AMB\0")]
#[derive(Debug, CopyGetters)]
pub struct SkyLight {
	/// `1` in every file the game ships.
	#[get_copy = "pub"]
	version: u16,

	#[br(temp, assert(kind == [0, 1]))]
	kind: [u8; 2],

	/// Zero in every file the game ships.
	#[get_copy = "pub"]
	unknown: u16,

	#[br(temp)]
	sky_count: u16,

	#[br(temp)]
	sample_count: u32,

	#[br(count = sky_count)]
	skies: Vec<Sky>,

	#[br(count = sample_count)]
	samples: Vec<Harmonics>,
}

impl SkyLight {
	/// Every sky this file carries.
	pub fn skies(&self) -> &[Sky] {
		&self.skies
	}

	/// The samples a sky holds, spread over the day.
	pub fn samples(&self, id: u16) -> Option<&[Harmonics]> {
		let sky = self.skies.iter().find(|sky| sky.id == id)?;
		let start = sky.first as usize;
		self.samples.get(start..start + sky.count as usize)
	}
}

/// Where in a [`SkyLight`] one sky's samples sit.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Sky {
	/// Names `bgcommon/nature/sky/texture/sky_<id>.tex`, the ID padded to three digits.
	id: u16,

	/// Samples this sky holds.
	count: u16,

	/// Index of the first of them.
	first: u32,
}

/// The light at one time of day.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Keyframe {
	light: Harmonics,

	/// Seconds since midnight.
	time: f32,
}

/// Light, as second order spherical harmonics with nine coefficients per colour channel. Each
/// channel runs constant, then `y`, `z`, `x`, then `xy`, `yz`, `3z^2 - 1`, `xz`, `x^2 - y^2`.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Harmonics {
	red: [f32; 9],
	green: [f32; 9],
	blue: [f32; 9],
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Ambient, EnvLocation, SkyLight, TRACK_COUNT};

	fn harmonics(seed: f32) -> impl Iterator<Item = u8> {
		(0..27).flat_map(move |index| (seed + index as f32).to_le_bytes())
	}

	fn env_location(counts: &[u32]) -> Vec<u8> {
		let mut slots = [0u32; TRACK_COUNT];
		slots[..counts.len()].copy_from_slice(counts);

		let mut bytes = Vec::new();
		bytes.extend(b"AMB\0");
		bytes.extend(1u16.to_le_bytes());
		bytes.extend([0, 0]);
		bytes.extend((0..9).flat_map(|index| (index as f32).to_le_bytes()));
		bytes.extend(slots.iter().flat_map(|count| count.to_le_bytes()));

		for keyframe in 0..slots.iter().sum::<u32>() {
			bytes.extend(harmonics(keyframe as f32 * 1000.));
			bytes.extend((keyframe as f32 * 3600.).to_le_bytes());
		}
		bytes
	}

	fn sky_light(skies: &[(u16, u16)]) -> Vec<u8> {
		let total = skies
			.iter()
			.map(|&(_, count)| u32::from(count))
			.sum::<u32>();

		let mut bytes = Vec::new();
		bytes.extend(b"AMB\0");
		bytes.extend(1u16.to_le_bytes());
		bytes.extend([0, 1]);
		bytes.extend(0u16.to_le_bytes());
		bytes.extend(u16::try_from(skies.len()).unwrap().to_le_bytes());
		bytes.extend(total.to_le_bytes());

		let mut first = 0u32;
		for &(id, count) in skies {
			bytes.extend(id.to_le_bytes());
			bytes.extend(count.to_le_bytes());
			bytes.extend(first.to_le_bytes());
			first += u32::from(count);
		}
		bytes.extend((0..total).flat_map(|sample| harmonics(sample as f32 * 1000.)));
		bytes
	}

	fn env(bytes: Vec<u8>) -> EnvLocation {
		match Ambient::read(Cursor::new(bytes)).unwrap() {
			Ambient::EnvLocation(location) => location,
			other => panic!("expected an env location, got {other:?}"),
		}
	}

	fn sky(bytes: Vec<u8>) -> SkyLight {
		match Ambient::read(Cursor::new(bytes)).unwrap() {
			Ambient::SkyLight(light) => light,
			other => panic!("expected a sky light, got {other:?}"),
		}
	}

	#[test]
	fn empty() {
		assert!(matches!(
			Ambient::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn an_unknown_kind_is_an_error() {
		let mut bytes = env_location(&[1]);
		bytes[7] = 2;
		assert!(matches!(
			Ambient::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	/// A track starts past every keyframe the tracks before it hold, empty tracks included.
	#[test]
	fn tracks_start_past_the_ones_before_them() {
		let file = env(env_location(&[2, 0, 3]));

		assert_eq!(file.sky_visibility(), [0., 1., 2., 3., 4., 5., 6., 7., 8.]);

		let track = file.track(0).unwrap();
		assert_eq!(track.len(), 2);
		assert_eq!(track[1].time(), 3600.);
		assert_eq!(track[1].light().red()[0], 1000.);

		assert!(file.track(1).unwrap().is_empty());

		let track = file.track(2).unwrap();
		assert_eq!(track.len(), 3);
		assert_eq!(track[0].time(), 7200.);
		assert_eq!(track[2].light().blue()[8], 4000. + 26.);

		assert!(file.track(TRACK_COUNT).is_none());
	}

	#[test]
	fn a_sky_reads_its_own_run_of_samples() {
		let file = sky(sky_light(&[(1, 2), (9, 1), (12, 3)]));

		assert_eq!(file.skies().len(), 3);
		assert_eq!(file.samples(1).unwrap().len(), 2);
		assert_eq!(file.samples(9).unwrap()[0].red()[0], 2000.);
		assert_eq!(file.samples(12).unwrap()[2].green()[0], 5000. + 9.);
		assert!(file.samples(2).is_none());
	}
}
