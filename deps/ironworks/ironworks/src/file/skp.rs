//! Structs and utilities for parsing .skp files.

use std::{fmt, io::SeekFrom, str};

use binrw::{BinRead, binread};
use getset::{CopyGetters, Getters};

use crate::{FileStream, error::Result};

use super::{animation, file::File};

pub use animation::AnimationLayer;

/// Parameters layered over the skeleton of the same name, covering the bones an animation drives,
/// how a chain of bones turns towards a look-at target, and how the body leans on a slope.
#[binread]
#[br(little, magic = b"plks")]
#[derive(Debug, Getters, CopyGetters)]
pub struct SkeletonParameters {
	/// Version of this parameter file. This is a XIV-specific version tag.
	#[get_copy = "pub"]
	version: Version,

	/// Which sections the file declares.
	#[br(map = Sections)]
	#[get_copy = "pub"]
	sections: Sections,

	#[br(temp)]
	header_size: u32,

	#[br(temp)]
	look_at_offset: u32,

	/// Offset to the cyclic coordinate descent section, which carries no known payload.
	#[get_copy = "pub"]
	ccd_offset: u32,

	/// Offset to the foot IK section, whose layout is unknown.
	#[get_copy = "pub"]
	foot_offset: u32,

	// Only the larger header carries this field.
	#[br(temp, if(header_size > 0x1c))]
	slope_offset: u32,

	/// Bones each animation layer drives. Empty where the file declares no animation section.
	#[br(
		if(sections.animation()),
		seek_before = SeekFrom::Start(header_size.into()),
		parse_with = animation::layers,
	)]
	#[get = "pub"]
	animation_layers: Vec<AnimationLayer>,

	#[br(if(sections.look_at()), seek_before = SeekFrom::Start(look_at_offset.into()))]
	look_at: Option<LookAt>,

	#[br(
		if(sections.slope() && slope_offset != 0),
		seek_before = SeekFrom::Start(slope_offset.into()),
	)]
	slope: Option<Slope>,
}

impl SkeletonParameters {
	/// Look-at setup, where the file declares one.
	pub fn look_at(&self) -> Option<&LookAt> {
		self.look_at.as_ref()
	}

	/// Slope response, where the file declares one.
	pub fn slope(&self) -> Option<&Slope> {
		self.slope.as_ref()
	}
}

impl File for SkeletonParameters {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// XIV skeleton parameter file version.
#[binread]
#[br(little)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Version([u8; 4]);

impl Version {
	/// The version read as a decimal number, or `None` if the tag is not one.
	pub fn number(&self) -> Option<u32> {
		let mut digits = self.0;
		digits.reverse();
		str::from_utf8(&digits).ok()?.parse().ok()
	}
}

impl fmt::Debug for Version {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.number() {
			Some(number) => write!(f, "{number}"),
			None => write!(f, "{:?}", self.0),
		}
	}
}

/// The sections a parameter file declares. Those carried appear in flag order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sections(u32);

impl Sections {
	const ANIMATION: u32 = 0x01;
	const LOOK_AT: u32 = 0x02;
	const CCD: u32 = 0x04;
	const FEET: u32 = 0x08;
	const SLOPE: u32 = 0x10;

	/// The flag word as written.
	pub fn bits(&self) -> u32 {
		self.0
	}

	/// Whether the file carries animation layers.
	pub fn animation(&self) -> bool {
		self.0 & Self::ANIMATION != 0
	}

	/// Whether the file carries a look-at setup.
	pub fn look_at(&self) -> bool {
		self.0 & Self::LOOK_AT != 0
	}

	/// Whether the file carries a cyclic coordinate descent section, which is not modelled here.
	pub fn ccd(&self) -> bool {
		self.0 & Self::CCD != 0
	}

	/// Whether the file carries a foot IK section, which is not modelled here.
	pub fn feet(&self) -> bool {
		self.0 & Self::FEET != 0
	}

	/// Whether the file carries a slope response.
	pub fn slope(&self) -> bool {
		self.0 & Self::SLOPE != 0
	}
}

impl fmt::Debug for Sections {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let names = [
			(Self::ANIMATION, "animation"),
			(Self::LOOK_AT, "look_at"),
			(Self::CCD, "ccd"),
			(Self::FEET, "feet"),
			(Self::SLOPE, "slope"),
		];
		let mut list = f.debug_list();
		for (mask, name) in names {
			if self.0 & mask != 0 {
				list.entry(&name);
			}
		}
		list.finish()
	}
}

/// How chains of bones turn towards a look-at target.
#[binread]
#[br(little)]
#[derive(Debug, Getters)]
pub struct LookAt {
	#[br(temp)]
	param_count: u8,

	#[br(temp)]
	group_count: u8,

	/// Parameter sets a group's elements select between.
	// The skipped bytes state each group's element count, which every group also states itself.
	#[br(pad_before = group_count, count = param_count)]
	#[get = "pub"]
	params: Vec<LookAtParam>,

	/// Chains of bones, each turning under one of the parameter sets.
	#[br(count = group_count)]
	#[get = "pub"]
	groups: Vec<LookAtGroup>,
}

/// One look-at parameter set.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct LookAtParam {
	/// How far the bone may turn, in radians.
	limit_angles: [f32; 4],

	/// In radians.
	forward_rotation: [f32; 3],

	/// In radians.
	limit_angle: f32,

	eye_positions: [f32; 3],

	flags: u32,

	/// How much of the turn towards the target is taken each step.
	gain: f32,

	/// This set's own position in the file's parameter list.
	#[br(pad_after = 3)]
	index: u8,
}

/// A named chain of bones driven by look-at.
#[binread]
#[br(little)]
#[derive(Debug, Getters, CopyGetters)]
pub struct LookAtGroup {
	#[get_copy = "pub"]
	id: Name,

	#[br(temp)]
	element_count: u8,

	///
	#[br(count = element_count)]
	#[get = "pub"]
	elements: Vec<LookAtGroupElement>,
}

/// One bone of a look-at chain.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct LookAtGroupElement {
	/// Order this bone turns in relative to the rest of its chain.
	priority: u8,

	/// Index into the file's look-at parameters.
	param_index: u8,

	bone_name: Name,

	parent_bone_name: Name,
}

/// How the body leans on a slope.
#[binread]
#[br(little, magic = b"psf\0\0\0\0\x02")]
#[derive(Debug, Getters, CopyGetters)]
pub struct Slope {
	#[get_copy = "pub"]
	unknown_a: i32,

	#[get_copy = "pub"]
	unknown_b: i32,

	#[get_copy = "pub"]
	unknown_c: u32,

	/// In radians.
	#[get_copy = "pub"]
	angles: [f32; 2],

	#[br(temp)]
	point_count: u32,

	///
	#[br(count = point_count)]
	#[get = "pub"]
	points: Vec<[f32; 3]>,
}

/// A fixed-width name. Buffers carry uninitialised bytes past the terminator.
#[binread]
#[br(little)]
#[derive(Clone, Copy)]
pub struct Name([u8; 32]);

impl Name {
	/// The name as written, or `None` if it is not text.
	pub fn as_str(&self) -> Option<&str> {
		let end = self
			.0
			.iter()
			.position(|byte| *byte == 0)
			.unwrap_or(self.0.len());
		str::from_utf8(&self.0[..end]).ok()
	}
}

impl fmt::Debug for Name {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.as_str() {
			Some(text) => write!(f, "{text:?}"),
			None => write!(f, "{:?}", self.0),
		}
	}
}

#[cfg(test)]
mod test {
	use std::{
		f32::consts::FRAC_PI_6,
		io::{self, Cursor},
	};

	use crate::{error::Error, file::File};

	use super::SkeletonParameters;

	/// A name buffer, filled past the terminator as the game's own are.
	fn name(text: &[u8]) -> [u8; 32] {
		let mut buffer = [0xcd; 32];
		buffer[..text.len()].copy_from_slice(text);
		buffer[text.len()] = 0;
		buffer
	}

	fn animation(layers: &[(u32, &[i16])]) -> Vec<u8> {
		let mut offsets = Vec::from(*b"hpla");
		offsets.extend(u16::try_from(layers.len()).unwrap().to_le_bytes());

		let mut bodies = Vec::new();
		let base = 6 + 2 * layers.len();
		for (layer, bones) in layers {
			offsets.extend(u16::try_from(base + bodies.len()).unwrap().to_le_bytes());
			bodies.extend(layer.to_le_bytes());
			bodies.extend(u16::try_from(bones.len()).unwrap().to_le_bytes());
			bodies.extend(bones.iter().flat_map(|bone| bone.to_le_bytes()));
		}

		offsets.extend(bodies);
		offsets
	}

	type Element<'a> = (u8, u8, &'a [u8], &'a [u8]);

	fn look_at(params: &[[f32; 4]], groups: &[(&[u8], &[Element])]) -> Vec<u8> {
		let mut bytes = vec![
			u8::try_from(params.len()).unwrap(),
			u8::try_from(groups.len()).unwrap(),
		];
		bytes.extend(
			groups
				.iter()
				.map(|(_, elements)| u8::try_from(elements.len()).unwrap()),
		);

		for (index, limit_angles) in params.iter().enumerate() {
			bytes.extend(limit_angles.iter().flat_map(|angle| angle.to_le_bytes()));
			bytes.extend([0; 12]); // forward_rotation
			bytes.extend([0; 4]); // limit_angle
			bytes.extend([0; 12]); // eye_positions
			bytes.extend(1u32.to_le_bytes()); // flags
			bytes.extend(1f32.to_le_bytes()); // gain
			bytes.push(u8::try_from(index).unwrap());
			bytes.extend([0; 3]);
		}

		for (id, elements) in groups {
			bytes.extend(name(id));
			bytes.push(u8::try_from(elements.len()).unwrap());
			for (priority, param_index, bone, parent) in *elements {
				bytes.extend([*priority, *param_index]);
				bytes.extend(name(bone));
				bytes.extend(name(parent));
			}
		}

		bytes
	}

	fn slope(angles: [f32; 2], points: &[[f32; 3]]) -> Vec<u8> {
		let mut bytes = Vec::from(*b"psf\0\0\0\0\x02");
		bytes.extend(80i32.to_le_bytes());
		bytes.extend(40i32.to_le_bytes());
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(angles.iter().flat_map(|angle| angle.to_le_bytes()));
		bytes.extend(u32::try_from(points.len()).unwrap().to_le_bytes());
		bytes.extend(points.iter().flatten().flat_map(|axis| axis.to_le_bytes()));
		bytes
	}

	fn skp(new: bool, sections: u32, animation: &[u8], look_at: &[u8], slope: &[u8]) -> Vec<u8> {
		let header_size = match new {
			true => 0x20,
			false => 0x1c,
		};
		let at = |section: &[u8], preceding: usize| match section.is_empty() {
			true => 0,
			false => header_size + preceding,
		};

		// The game aligns the slope block to four, padding out the section before it.
		let before_slope = header_size + animation.len() + look_at.len();
		let padding = match slope.is_empty() {
			true => 0,
			false => before_slope.next_multiple_of(4) - before_slope,
		};
		let size = before_slope + padding + slope.len();

		let mut bytes = Vec::from(*b"plks");
		bytes.extend(match new {
			true => *b"0031",
			false => *b"0001",
		});
		bytes.extend(sections.to_le_bytes());
		for field in [
			header_size,
			at(look_at, animation.len()),
			match sections & 0x04 {
				0 => 0,
				_ => size,
			},
			0,
		] {
			bytes.extend(u32::try_from(field).unwrap().to_le_bytes());
		}
		if new {
			let offset = at(slope, animation.len() + look_at.len() + padding);
			bytes.extend(u32::try_from(offset).unwrap().to_le_bytes());
		}

		bytes.extend(animation);
		bytes.extend(look_at);
		bytes.extend(vec![0; padding]);
		bytes.extend(slope);
		bytes
	}

	fn head_group() -> Vec<u8> {
		look_at(
			&[[0.05, -0.17, 0.2, -0.2]],
			&[(
				b"base",
				&[(0, 0, b"j_kubi", b"j_sebo_b"), (1, 0, b"j_kao", b"j_kubi")],
			)],
		)
	}

	#[test]
	fn empty() {
		assert!(matches!(
			SkeletonParameters::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn the_smaller_header_carries_animation_layers() {
		let file = SkeletonParameters::read(Cursor::new(skp(
			false,
			0x01,
			&animation(&[(19, &[0]), (1, &[1, 2, 3])]),
			&[],
			&[],
		)))
		.unwrap();

		assert_eq!(file.version().number(), Some(1000));
		assert_eq!(file.animation_layers().len(), 2);
		assert_eq!(file.animation_layers()[1].layer(), 1);
		assert_eq!(file.animation_layers()[1].bone_indices(), &[1, 2, 3]);
		assert!(file.look_at().is_none());
		assert!(file.slope().is_none());
	}

	/// Layers are reached through the block's offset table, which need not run in order.
	#[test]
	fn animation_layers_follow_their_offsets() {
		let mut block = Vec::from(*b"hpla");
		block.extend(2u16.to_le_bytes());
		block.extend(18u16.to_le_bytes());
		block.extend(10u16.to_le_bytes());
		for (layer, bone) in [(7u32, 11i16), (8, 12)] {
			block.extend(layer.to_le_bytes());
			block.extend(1u16.to_le_bytes());
			block.extend(bone.to_le_bytes());
		}

		let file =
			SkeletonParameters::read(Cursor::new(skp(false, 0x01, &block, &[], &[]))).unwrap();
		assert_eq!(file.animation_layers()[0].bone_indices(), &[12]);
		assert_eq!(file.animation_layers()[1].bone_indices(), &[11]);
	}

	#[test]
	fn the_larger_header_carries_look_at() {
		let file = SkeletonParameters::read(Cursor::new(skp(true, 0x02, &[], &head_group(), &[])))
			.unwrap();

		assert_eq!(file.version().number(), Some(1300));
		assert!(file.animation_layers().is_empty());

		let look_at = file.look_at().unwrap();
		assert_eq!(look_at.params().len(), 1);
		assert_eq!(look_at.params()[0].limit_angles(), [0.05, -0.17, 0.2, -0.2]);
		assert_eq!(look_at.params()[0].gain(), 1.0);
		assert_eq!(look_at.params()[0].index(), 0);

		let group = &look_at.groups()[0];
		assert_eq!(group.id().as_str(), Some("base"));
		assert_eq!(group.elements().len(), 2);
		assert_eq!(group.elements()[1].priority(), 1);
		assert_eq!(group.elements()[1].bone_name().as_str(), Some("j_kao"));
		assert_eq!(
			group.elements()[1].parent_bone_name().as_str(),
			Some("j_kubi")
		);
	}

	#[test]
	fn look_at_and_slope_sit_at_their_own_offsets() {
		// A single element leaves the look-at section ending two short of the slope block.
		let sections = look_at(&[[0.0; 4]], &[(b"base", &[(0, 0, b"j_kubi", b"j_sebo_b")])]);
		let points = [[0.0, 0.0, 1.9], [0.0, 0.0, -1.0]];
		let file = SkeletonParameters::read(Cursor::new(skp(
			true,
			0x12,
			&[],
			&sections,
			&slope([FRAC_PI_6, 0.0], &points),
		)))
		.unwrap();

		assert_eq!(file.look_at().unwrap().groups()[0].elements().len(), 1);

		let slope = file.slope().unwrap();
		assert_eq!(slope.unknown_a(), 80);
		assert_eq!(slope.angles(), [FRAC_PI_6, 0.0]);
		assert_eq!(slope.points()[1], points[1]);
	}

	#[test]
	fn slope_stands_alone() {
		let file = SkeletonParameters::read(Cursor::new(skp(
			true,
			0x10,
			&[],
			&[],
			&slope([0.34907, 0.17453], &[[1.0, 2.0, 3.0]]),
		)))
		.unwrap();

		assert!(file.look_at().is_none());
		assert_eq!(file.slope().unwrap().points(), &[[1.0, 2.0, 3.0]]);
	}

	/// The smaller header stops before the slope offset, leaving the section unreachable.
	#[test]
	fn the_smaller_header_cannot_reach_a_slope() {
		let file = SkeletonParameters::read(Cursor::new(skp(
			false,
			0x11,
			&animation(&[(1, &[0])]),
			&[],
			&[],
		)))
		.unwrap();

		assert!(file.sections().slope());
		assert!(file.slope().is_none());
	}

	#[test]
	fn ccd_points_past_the_last_section() {
		let bytes = skp(true, 0x06, &[], &head_group(), &[]);
		let size = u32::try_from(bytes.len()).unwrap();

		let file = SkeletonParameters::read(Cursor::new(bytes)).unwrap();
		assert!(file.sections().ccd());
		assert_eq!(file.ccd_offset(), size);
		assert!(file.look_at().is_some());
	}

	#[test]
	fn truncated() {
		let mut bytes = skp(true, 0x02, &[], &head_group(), &[]);
		bytes.truncate(bytes.len() - 8);

		assert!(matches!(
			SkeletonParameters::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
