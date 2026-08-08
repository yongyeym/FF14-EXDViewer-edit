use std::{fmt, str};

use binrw::binread;

#[binread]
#[br(little)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Version([u8; 4]);

impl Version {
	/// The tag as written, or `None` if it is not text.
	pub fn as_str(&self) -> Option<&str> {
		str::from_utf8(&self.0).ok()
	}

	/// The tag read as a decimal number, or `None` if it is not one.
	pub fn number(&self) -> Option<u32> {
		self.as_str()?.parse().ok()
	}
}

impl fmt::Debug for Version {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.as_str() {
			Some(text) => write!(f, "{text:?}"),
			None => write!(f, "{:?}", self.0),
		}
	}
}

/// The file's root header.
#[binread]
#[br(little, magic = b"uldh")]
#[derive(Debug)]
pub struct Header {
	pub version: Version,
	/// Offset from the start of the file to the section carrying the asset, part, component and
	/// timeline lists.
	pub resource_section_offset: u32,
	/// Offset from the start of the file to the section carrying the widget list.
	pub widget_section_offset: u32,
}

/// A section header. A file carries two, and each list belongs to whichever one declares it.
#[binread]
#[br(little, magic = b"atkh")]
#[derive(Debug)]
pub struct Section {
	_version: Version,
	// Every offset below is relative to the start of this header, not to the file.
	pub asset_list_offset: u32,
	pub part_list_offset: u32,
	pub component_list_offset: u32,
	pub timeline_list_offset: u32,
	pub widget_list_offset: u32,
	_rewrite_data_offset: u32,
	/// Lumina reads this as the timeline list's size and Physis as a timeline count. Files agree
	/// with neither -- one carrying 31 timelines writes 34 -- so it is kept as written.
	_unknown: u32,
}

/// The header every list starts with.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct ListHeader {
	pub magic: [u8; 4],
	pub version: Version,
	pub count: u32,
	_unknown: i32,
}

impl ListHeader {
	pub const SIZE: usize = 16;

	pub const ASSETS: &'static [u8; 4] = b"ashd";
	pub const PARTS: &'static [u8; 4] = b"tphd";
	pub const COMPONENTS: &'static [u8; 4] = b"cohd";
	pub const TIMELINES: &'static [u8; 4] = b"tlhd";
	pub const WIDGETS: &'static [u8; 4] = b"wdhd";
}

/// One entry of the asset list.
#[binread]
#[br(little, import(themed: bool))]
#[derive(Debug)]
pub struct TextureEntry {
	pub id: u32,
	pub path: [u8; 44],
	pub icon_id: u32,
	/// Written from `ashd` version 0101 onwards; entries without it are four bytes shorter.
	#[br(if(themed))]
	pub theme_bitmask: Option<u8>,
}

impl TextureEntry {
	/// Stride of an entry, which the asset list's own version decides.
	pub fn stride(themed: bool) -> usize {
		match themed {
			true => 56,
			false => 52,
		}
	}
}

/// One entry of the part list. `offset` is the whole record's size, the parts included.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct PartList {
	pub id: u32,
	pub part_count: u32,
	pub offset: u32,
}

impl PartList {
	pub const SIZE: usize = 12;
}

/// One entry of the component list. Unlike [`PartList`], `offset` points at the node list rather
/// than past the record; `size` is what covers the whole thing.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Component {
	pub id: u32,
	pub ignore_input: u8,
	pub drag_arrow: u8,
	pub drop_arrow: u8,
	pub kind: u8,
	pub node_count: u32,
	/// Total size of the record, the nodes included.
	pub size: u16,
	/// Where the node list starts, relative to the record.
	pub offset: u16,
}

impl Component {
	pub const SIZE: usize = 16;
}

/// One entry of the widget list. Unlike [`Component`], the nodes follow the header directly and
/// `offset` is the whole record's size, as it is for [`PartList`] and [`Timeline`].
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Widget {
	pub id: u32,
	pub alignment: u8,
	#[br(pad_after = 2)]
	pub themed_assets: u8,
	pub x: i16,
	pub y: i16,
	pub node_count: u16,
	pub offset: u16,
}

impl Widget {
	pub const SIZE: usize = 16;
}

/// The fixed prefix every node starts with. `node_offset` is the whole node's size, so the payload
/// that follows runs to `node_offset - SIZE` bytes.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Node {
	pub node_id: u32,
	pub parent_id: i32,
	pub next_sibling_id: i32,
	pub previous_sibling_id: i32,
	pub child_node_id: i32,
	pub node_type: i32,
	pub node_offset: u16,
	pub tab_index: i16,
	pub unknown1: [i32; 4],
	pub x: i16,
	pub y: i16,
	pub width: u16,
	pub height: u16,
	pub rotation: f32,
	pub scale_x: f32,
	pub scale_y: f32,
	pub origin_x: i16,
	pub origin_y: i16,
	pub priority: u16,
	pub flags: u16,
	pub multiply_red: i16,
	pub multiply_green: i16,
	pub multiply_blue: i16,
	pub add_red: i16,
	pub add_green: i16,
	pub add_blue: i16,
	pub alpha: u8,
	pub clip_count: u8,
	pub timeline_id: u16,
}

impl Node {
	pub const SIZE: usize = 88;
}

/// One entry of the timeline list. `offset` is the whole record's size, the frames included.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Timeline {
	pub id: u32,
	pub offset: u32,
	pub frame_count_1: u16,
	pub frame_count_2: u16,
}

impl Timeline {
	pub const SIZE: usize = 12;
}

/// `offset` is the whole record's size, the key groups included.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Frame {
	pub start_frame: u32,
	pub end_frame: u32,
	pub offset: u32,
	pub keygroup_count: u32,
}

impl Frame {
	pub const SIZE: usize = 16;
}

/// `offset` is the whole record's size, the keyframes included.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct KeyGroup {
	pub usage: u16,
	pub kind: u16,
	pub offset: u16,
	pub keyframe_count: u16,
}

impl KeyGroup {
	pub const SIZE: usize = 8;
}

#[cfg(test)]
mod test {
	use super::Version;

	/// The asset list's entry stride hangs off this, so a tag that is not a number has to be
	/// distinguishable from one that is rather than defaulting.
	#[test]
	fn version_reads_as_a_number_only_when_it_is_one() {
		assert_eq!(Version(*b"0100").number(), Some(100));
		assert_eq!(Version(*b"0101").number(), Some(101));
		assert_eq!(Version(*b"abcd").number(), None);
		assert_eq!(Version([0xFF; 4]).number(), None);
		assert_eq!(Version(*b"0100").as_str(), Some("0100"));
	}
}
