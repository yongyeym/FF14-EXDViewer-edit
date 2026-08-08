use binrw::BinRead;
use derivative::Derivative;
use getset::CopyGetters;

use crate::error::Result;

use super::{cursor, invalid, region, structs};

/// An animation resource, referenced by the nodes it drives through [`Node::timeline_id`](super::Node).
#[derive(Debug, CopyGetters)]
pub struct Timeline {
	/// Identifier, unique across the file.
	#[get_copy = "pub"]
	id: u32,

	animations: Vec<Animation>,

	label_sets: Vec<Animation>,
}

impl Timeline {
	/// The animations driving node properties, in file order.
	pub fn animations(&self) -> &[Animation] {
		&self.animations
	}

	/// The label sets, which name the points an animation can be told to jump to. They take the same
	/// shape as an animation, carrying [`KeyGroupKind::Label`] groups in place of property ones.
	pub fn label_sets(&self) -> &[Animation] {
		&self.label_sets
	}
}

/// A span of frames and what drives it over that span.
#[derive(Debug, CopyGetters)]
pub struct Animation {
	#[get_copy = "pub"]
	start_frame: u32,

	#[get_copy = "pub"]
	end_frame: u32,

	groups: Vec<KeyGroup>,
}

impl Animation {
	/// The key groups active over this span.
	pub fn groups(&self) -> &[KeyGroup] {
		&self.groups
	}
}

/// Keyframes driving one property of a node.
#[derive(Derivative, CopyGetters)]
#[derivative(Debug)]
pub struct KeyGroup {
	/// Which property is driven.
	#[get_copy = "pub"]
	usage: KeyUsage,

	/// What shape the keyframes take. ironworks does not decode them.
	#[get_copy = "pub"]
	kind: KeyGroupKind,

	#[get_copy = "pub"]
	keyframe_count: u16,

	#[derivative(Debug = "ignore")]
	data: Vec<u8>,
}

impl KeyGroup {
	/// The keyframe block as written.
	pub fn data(&self) -> &[u8] {
		&self.data
	}

	/// Bytes per keyframe, where the block divides evenly by the declared count. `None` where the
	/// count is zero.
	pub fn keyframe_size(&self) -> Option<usize> {
		let count = usize::from(self.keyframe_count);
		(count > 0 && self.data.len() % count == 0).then(|| self.data.len() / count)
	}
}

/// The property a key group drives.
///
/// The last two depend on the kind of node being driven: on text they are its colours, and on an
/// image or nine-grid the first selects which part is shown. A label group drives no property and
/// carries zero here, so it reads as [`KeyUsage::Position`] without meaning it.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyUsage {
	Position,
	Rotation,
	Scale,
	Alpha,
	NodeColor,
	TextColor,
	EdgeColor,
	/// A usage ironworks does not recognise; the inner value is the raw tag.
	Unknown(u16),
}

impl From<u16> for KeyUsage {
	fn from(value: u16) -> Self {
		match value {
			0x0 => Self::Position,
			0x1 => Self::Rotation,
			0x2 => Self::Scale,
			0x3 => Self::Alpha,
			0x4 => Self::NodeColor,
			0x5 => Self::TextColor,
			0x6 => Self::EdgeColor,
			other => Self::Unknown(other),
		}
	}
}

/// What shape a key group's values take.
///
/// The game collapses these into the eight cases its runtime union can hold, so several map onto one
/// there: a `Byte1` becomes a byte and a `Byte3` a colour, both four bytes wide on disk. Eight of the
/// twenty-six appear in the shipped layouts -- `Float1`, `Float2`, `Byte1`, `Byte3`, `UShort1`,
/// `UInt1`, `Color` and `Label`.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyGroupKind {
	Float1,
	Float2,
	Float3,
	SByte1,
	SByte2,
	SByte3,
	Byte1,
	Byte2,
	Byte3,
	Short1,
	Short2,
	Short3,
	UShort1,
	UShort2,
	UShort3,
	Int1,
	Int2,
	Int3,
	UInt1,
	UInt2,
	UInt3,
	Bool1,
	Bool2,
	Bool3,
	Color,
	Label,
	/// A kind ironworks does not recognise; the inner value is the raw tag.
	Unknown(u16),
}

impl From<u16> for KeyGroupKind {
	fn from(value: u16) -> Self {
		match value {
			0x00 => Self::Float1,
			0x01 => Self::Float2,
			0x02 => Self::Float3,
			0x03 => Self::SByte1,
			0x04 => Self::SByte2,
			0x05 => Self::SByte3,
			0x06 => Self::Byte1,
			0x07 => Self::Byte2,
			0x08 => Self::Byte3,
			0x09 => Self::Short1,
			0x0A => Self::Short2,
			0x0B => Self::Short3,
			0x0C => Self::UShort1,
			0x0D => Self::UShort2,
			0x0E => Self::UShort3,
			0x0F => Self::Int1,
			0x10 => Self::Int2,
			0x11 => Self::Int3,
			0x12 => Self::UInt1,
			0x13 => Self::UInt2,
			0x14 => Self::UInt3,
			0x15 => Self::Bool1,
			0x16 => Self::Bool2,
			0x17 => Self::Bool3,
			0x18 => Self::Color,
			0x19 => Self::Label,
			other => Self::Unknown(other),
		}
	}
}

pub(super) fn read_timelines(bytes: &[u8], at: usize, count: u32) -> Result<Vec<Timeline>> {
	let mut timelines = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let head = structs::Timeline::read(&mut cursor(bytes, at)?)?;
		let size = usize::try_from(head.offset).expect("u32 fits usize");
		region(bytes, at, size, structs::Timeline::SIZE)?;

		let end = at + size;
		let animation_count = usize::from(head.frame_count_1);
		let total = animation_count + usize::from(head.frame_count_2);
		let mut animations = Vec::new();
		let mut label_sets = Vec::new();
		let mut record_at = at + structs::Timeline::SIZE;
		for index in 0..total {
			let (animation, record_end) = read_animation(bytes, record_at)?;
			if record_end > end {
				return Err(invalid(format!(
					"timeline at {at:#x} has records running {} bytes past the record",
					record_end - end
				)));
			}
			match index < animation_count {
				true => animations.push(animation),
				false => label_sets.push(animation),
			}
			record_at = record_end;
		}

		timelines.push(Timeline {
			id: head.id,
			animations,
			label_sets,
		});
		at += size;
	}
	Ok(timelines)
}

fn read_animation(bytes: &[u8], at: usize) -> Result<(Animation, usize)> {
	let head = structs::Frame::read(&mut cursor(bytes, at)?)?;
	let size = usize::try_from(head.offset).expect("u32 fits usize");
	region(bytes, at, size, structs::Frame::SIZE)?;

	let end = at + size;
	let mut groups = Vec::new();
	let mut group_at = at + structs::Frame::SIZE;
	for _ in 0..head.keygroup_count {
		let group = structs::KeyGroup::read(&mut cursor(bytes, group_at)?)?;
		let group_size = usize::from(group.offset);
		let data = region(bytes, group_at, group_size, structs::KeyGroup::SIZE)?;
		if group_at + group_size > end {
			return Err(invalid(format!(
				"animation at {at:#x} has key groups running {} bytes past the record",
				group_at + group_size - end
			)));
		}
		groups.push(KeyGroup {
			usage: group.usage.into(),
			kind: group.kind.into(),
			keyframe_count: group.keyframe_count,
			data: data.to_vec(),
		});
		group_at += group_size;
	}

	let animation = Animation {
		start_frame: head.start_frame,
		end_frame: head.end_frame,
		groups,
	};
	Ok((animation, at + size))
}
