use getset::CopyGetters;

use crate::error::Result;

use super::invalid;

/// Bytes one keyframe occupies.
const KEY: usize = 16;

/// One keyframe of a curve.
///
/// A curve is any block carrying a `Keys` list. Beside it sit `BvPr` and `BvPo`, the
/// [behaviours](CurveBehaviour) either side of the keyed span, and `RanT`, the
/// [randomisation](RandomKind) applied to the result.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct CurveKey {
	/// When the key sits, in frames. Nothing in the file states the rate frames are counted at.
	time: i16,

	/// How the span up to the next key is interpolated.
	kind: KeyKind,

	/// The three floats the key carries. The last is the value the curve takes; VFXEditor reads
	/// the first two as the tangent scales of a spline segment, and they carry `1.0` on almost
	/// every key that is not a [spline](KeyKind::Spline).
	data: [f32; 3],
}

impl CurveKey {
	/// The value the curve takes at this key.
	pub fn value(&self) -> f32 {
		self.data[2]
	}

	pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Self>> {
		if bytes.len() % KEY != 0 {
			return Err(invalid(format!(
				"key list of {} bytes does not divide into keys",
				bytes.len()
			)));
		}

		Ok(bytes
			.chunks_exact(KEY)
			.map(|key| Self {
				time: i16::from_le_bytes(key[0..2].try_into().unwrap()),
				kind: i16::from_le_bytes(key[2..4].try_into().unwrap()).into(),
				data: [
					f32::from_le_bytes(key[4..8].try_into().unwrap()),
					f32::from_le_bytes(key[8..12].try_into().unwrap()),
					f32::from_le_bytes(key[12..16].try_into().unwrap()),
				],
			})
			.collect())
	}
}

/// How a keyframe reaches the one after it.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
	Spline,
	Linear,
	Step,
	/// A kind ironworks does not recognise; the inner value is the raw tag.
	Unknown(i16),
}

impl From<i16> for KeyKind {
	fn from(value: i16) -> Self {
		match value {
			0 => Self::Spline,
			1 => Self::Linear,
			2 => Self::Step,
			other => Self::Unknown(other),
		}
	}
}

/// What a curve does either side of its keyed span, held in `BvPr` ahead of it and `BvPo` after.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveBehaviour {
	None,
	Constant,
	Repeat,
	Add,
	/// A behaviour ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for CurveBehaviour {
	fn from(value: i32) -> Self {
		match value {
			-1 => Self::None,
			0 => Self::Constant,
			1 => Self::Repeat,
			2 => Self::Add,
			other => Self::Unknown(other),
		}
	}
}

/// How a curve's value is randomised, held in `RanT` beside it.
///
/// The first three draw once, the last three on every evaluation.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomKind {
	FirstPlusMinus,
	FirstPlus,
	FirstMinus,
	AlwaysPlusMinus,
	AlwaysPlus,
	AlwaysMinus,
	/// A kind ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for RandomKind {
	fn from(value: i32) -> Self {
		match value {
			0 => Self::FirstPlusMinus,
			1 => Self::FirstPlus,
			2 => Self::FirstMinus,
			3 => Self::AlwaysPlusMinus,
			4 => Self::AlwaysPlus,
			5 => Self::AlwaysMinus,
			other => Self::Unknown(other),
		}
	}
}
