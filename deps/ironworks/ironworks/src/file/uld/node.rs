use std::{fmt, io::Cursor};

use binrw::{BinRead, binread};
use derivative::Derivative;
use getset::CopyGetters;

use crate::error::Result;

use super::{cursor, region, structs};

const COMPONENT_BASE: i32 = 1000;

/// A node in a widget's or component's tree.
#[derive(Derivative, CopyGetters)]
#[derivative(Debug)]
pub struct Node {
	/// Identifier, unique within the tree this node belongs to but not within the file.
	#[get_copy = "pub"]
	id: u32,

	/// The node this one hangs off, or zero for a root.
	#[get_copy = "pub"]
	parent_id: i32,

	/// The next node at this level, or zero at the end.
	#[get_copy = "pub"]
	next_sibling_id: i32,

	/// The previous node at this level, or zero at the start.
	#[get_copy = "pub"]
	previous_sibling_id: i32,

	/// The first of this node's children to be drawn, which is the last of them in the file.
	#[get_copy = "pub"]
	child_node_id: i32,

	/// The type tag as written. [`kind`](Self::kind) is the decoded form.
	#[get_copy = "pub"]
	node_type: i32,

	/// Position within the parent, in pixels.
	#[get_copy = "pub"]
	x: i16,
	/// Position within the parent, in pixels.
	#[get_copy = "pub"]
	y: i16,

	/// Size in pixels.
	#[get_copy = "pub"]
	width: u16,
	/// Size in pixels.
	#[get_copy = "pub"]
	height: u16,

	#[get_copy = "pub"]
	rotation: f32,
	#[get_copy = "pub"]
	scale_x: f32,
	#[get_copy = "pub"]
	scale_y: f32,

	/// The point rotation and scale are applied about, in pixels from the node's origin.
	#[get_copy = "pub"]
	origin_x: i16,
	#[get_copy = "pub"]
	origin_y: i16,

	/// Zero in every shipped layout; siblings are ordered by their links instead.
	#[get_copy = "pub"]
	priority: u16,

	/// Keyboard navigation order.
	#[get_copy = "pub"]
	tab_index: i16,

	#[get_copy = "pub"]
	flags: NodeFlags,

	/// Per-channel tint, where 100 leaves the channel unchanged.
	#[get_copy = "pub"]
	multiply: [i16; 3],

	/// Per-channel offset, where 0 leaves the channel unchanged.
	#[get_copy = "pub"]
	add: [i16; 3],

	/// Opacity, where 255 is fully opaque.
	#[get_copy = "pub"]
	alpha: u8,

	#[get_copy = "pub"]
	clip_count: u8,

	/// The timeline animating this node, looked up on the *file* rather than on this tree; unlike
	/// node ids, timeline ids are unique across the whole `.uld`.
	#[get_copy = "pub"]
	timeline_id: u16,

	/// Four words of the prefix whose meaning is unidentified.
	#[get_copy = "pub"]
	unknown: [i32; 4],

	kind: NodeKind,

	#[derivative(Debug = "ignore")]
	trailing: Vec<u8>,
}

impl Node {
	pub fn kind(&self) -> &NodeKind {
		&self.kind
	}

	/// Bytes of the node's payload that ironworks did not decode. Empty where the payload was
	/// understood in full, and for a component instance the part of it that depends on which
	/// component is being instanced.
	pub fn trailing(&self) -> &[u8] {
		&self.trailing
	}

	fn new(node: structs::Node, payload: &[u8]) -> Self {
		let (kind, used) = decode(node.node_type, payload).unwrap_or_else(|| {
			let kind = NodeKind::Unknown {
				node_type: node.node_type,
				data: payload.to_vec(),
			};
			(kind, payload.len())
		});

		Self {
			id: node.node_id,
			parent_id: node.parent_id,
			next_sibling_id: node.next_sibling_id,
			previous_sibling_id: node.previous_sibling_id,
			child_node_id: node.child_node_id,
			node_type: node.node_type,
			x: node.x,
			y: node.y,
			width: node.width,
			height: node.height,
			rotation: node.rotation,
			scale_x: node.scale_x,
			scale_y: node.scale_y,
			origin_x: node.origin_x,
			origin_y: node.origin_y,
			priority: node.priority,
			tab_index: node.tab_index,
			flags: NodeFlags(node.flags),
			multiply: [node.multiply_red, node.multiply_green, node.multiply_blue],
			add: [node.add_red, node.add_green, node.add_blue],
			alpha: node.alpha,
			clip_count: node.clip_count,
			timeline_id: node.timeline_id,
			unknown: node.unknown1,
			kind,
			trailing: payload[used..].to_vec(),
		}
	}
}

/// Decode a payload, returning it and how much of the region it consumed. `None` when the type is
/// unmodelled, or when its payload did not fit the region the record set aside for it.
fn decode(node_type: i32, payload: &[u8]) -> Option<(NodeKind, usize)> {
	let mut cursor = Cursor::new(payload);
	let kind = match node_type {
		1 => NodeKind::Res,
		2 => NodeKind::Image(Image::read(&mut cursor).ok()?),
		3 => NodeKind::Text(Text::read(&mut cursor).ok()?),
		4 => NodeKind::NineGrid(NineGrid::read(&mut cursor).ok()?),
		5 => NodeKind::Counter(Counter::read(&mut cursor).ok()?),
		8 => NodeKind::Collision(Collision::read(&mut cursor).ok()?),
		10 => NodeKind::ClippingMask(ClippingMask::read(&mut cursor).ok()?),
		id if id >= COMPONENT_BASE => NodeKind::Component {
			// The tag is the id of the component being instanced.
			component_id: u32::try_from(id).ok()?,
			instance: ComponentInstance::read(&mut cursor).ok()?,
		},
		_ => return None,
	};
	Some((kind, usize::try_from(cursor.position()).ok()?))
}

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub enum NodeKind {
	Res,
	Image(Image),
	Text(Text),
	NineGrid(NineGrid),
	Counter(Counter),
	Collision(Collision),
	ClippingMask(ClippingMask),
	/// An instance of another component. What follows [`ComponentInstance`] in the payload depends
	/// on that component's kind and is left in [`Node::trailing`].
	Component {
		component_id: u32,
		instance: ComponentInstance,
	},
	/// A payload ironworks does not model.
	Unknown {
		node_type: i32,
		#[derivative(Debug = "ignore")]
		data: Vec<u8>,
	},
}

/// A sprite cut from a texture atlas.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct Image {
	/// The part list to take the sprite from.
	pub part_list_id: u32,
	/// Index of the part within that list.
	pub part_id: u32,
	pub flip_horizontal: u8,
	pub flip_vertical: u8,
	pub wrap: u8,
	pub unknown: u8,
}

/// A run of text, whose content comes from a sheet rather than the layout.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct Text {
	/// Row of the sheet named by [`sheet_type`](Self::sheet_type) holding the text to draw.
	pub text_id: u32,
	pub color: u32,
	/// Where in the node the text sits; one of the nine [`Alignment`](super::Alignment) values.
	#[br(pad_after = 1)]
	pub alignment: u8,
	#[br(map = |raw: u8| Font::from(raw))]
	pub font: Font,
	pub font_size: u8,
	pub edge_color: u32,
	#[br(map = |raw: u8| TextFlags(raw))]
	pub flags: TextFlags,
	/// Which sheet [`text_id`](Self::text_id) indexes: 0 Addon, 1 Lobby.
	pub sheet_type: u8,
	pub char_spacing: u8,
	pub line_spacing: u8,
	/// Bit 1 draws the fill in the current UI theme's colour, bit 2 the edge.
	#[br(pad_after = 3)]
	pub flags2: u8,
}

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
	Axis,
	MiedingerMed,
	Miedinger,
	TrumpGothic,
	Jupiter,
	JupiterLarge,
	/// A font ironworks does not recognise; the inner value is the raw tag.
	Unknown(u8),
}

impl From<u8> for Font {
	fn from(value: u8) -> Self {
		match value {
			0 => Self::Axis,
			1 => Self::MiedingerMed,
			2 => Self::Miedinger,
			3 => Self::TrumpGothic,
			4 => Self::Jupiter,
			5 => Self::JupiterLarge,
			other => Self::Unknown(other),
		}
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TextFlags(u8);

impl TextFlags {
	const BOLD: u8 = 0x01;
	const ITALIC: u8 = 0x02;
	const EDGE: u8 = 0x04;
	const GLARE: u8 = 0x08;
	const MULTILINE: u8 = 0x10;
	const ELLIPSIS: u8 = 0x20;
	const WORD_WRAP: u8 = 0x40;
	const EMBOSS: u8 = 0x80;

	/// The flag byte as written.
	pub fn bits(&self) -> u8 {
		self.0
	}

	/// Never set by any layout the game ships; weight comes from the font instead.
	pub fn bold(&self) -> bool {
		self.0 & Self::BOLD != 0
	}

	pub fn italic(&self) -> bool {
		self.0 & Self::ITALIC != 0
	}

	/// Whether the glyphs are outlined.
	pub fn edge(&self) -> bool {
		self.0 & Self::EDGE != 0
	}

	pub fn glare(&self) -> bool {
		self.0 & Self::GLARE != 0
	}

	pub fn multiline(&self) -> bool {
		self.0 & Self::MULTILINE != 0
	}

	/// Whether text too long for the node is truncated with an ellipsis.
	pub fn ellipsis(&self) -> bool {
		self.0 & Self::ELLIPSIS != 0
	}

	pub fn word_wrap(&self) -> bool {
		self.0 & Self::WORD_WRAP != 0
	}

	pub fn emboss(&self) -> bool {
		self.0 & Self::EMBOSS != 0
	}
}

impl fmt::Debug for TextFlags {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let names = [
			(Self::BOLD, "bold"),
			(Self::ITALIC, "italic"),
			(Self::EDGE, "edge"),
			(Self::GLARE, "glare"),
			(Self::MULTILINE, "multiline"),
			(Self::ELLIPSIS, "ellipsis"),
			(Self::WORD_WRAP, "word_wrap"),
			(Self::EMBOSS, "emboss"),
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

/// A sprite stretched by its middle, keeping its corners and edges intact.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct NineGrid {
	pub part_list_id: u32,
	pub part_id: u32,
	pub parts_type: u8,
	pub render_type: u8,
	/// How far in from each edge the stretchable middle starts.
	pub top_offset: i16,
	pub bottom_offset: i16,
	pub left_offset: i16,
	pub right_offset: i16,
	pub blend_mode: u8,
	pub unknown: u8,
}

/// A number rendered from digit sprites.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct Counter {
	pub part_list_id: u32,
	pub part_id: u8,
	pub number_width: u8,
	pub comma_width: u8,
	pub space_width: u8,
	pub alignment: u16,
	pub unknown: u16,
}

/// A sprite used as a stencil, masking what its siblings draw.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
pub struct ClippingMask {
	/// The part list to take the mask from.
	pub part_list_id: u32,
	/// Index of the part within that list.
	pub part_id: u32,
}

/// An invisible region that takes input.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct Collision {
	pub kind: u16,
	pub unknown: u16,
	pub x: i32,
	pub y: i32,
	pub radius: u32,
}

/// The type-independent part of a component instance's payload: where focus moves from here.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
#[allow(missing_docs)]
pub struct ComponentInstance {
	pub index: u8,
	pub up: u8,
	pub down: u8,
	pub left: u8,
	pub right: u8,
	pub cursor: u8,
	pub flags: u8,
	pub unknown: u8,
	pub offset_x: i16,
	pub offset_y: i16,
}

/// Behaviour flags on a node.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NodeFlags(u16);

impl NodeFlags {
	const VISIBLE: u16 = 0x0001;
	const ENABLED: u16 = 0x0002;
	const CLIP: u16 = 0x0004;
	const FILL: u16 = 0x0008;
	const ANCHOR_TOP: u16 = 0x0010;
	const ANCHOR_BOTTOM: u16 = 0x0020;
	const ANCHOR_LEFT: u16 = 0x0040;
	const ANCHOR_RIGHT: u16 = 0x0080;
	const HAS_COLLISION: u16 = 0x0100;

	/// The flag word as written.
	pub fn bits(&self) -> u16 {
		self.0
	}

	/// Whether the node is drawn.
	pub fn visible(&self) -> bool {
		self.0 & Self::VISIBLE != 0
	}

	/// Whether the node takes input.
	pub fn enabled(&self) -> bool {
		self.0 & Self::ENABLED != 0
	}

	/// Whether children are clipped to this node's bounds.
	pub fn clip(&self) -> bool {
		self.0 & Self::CLIP != 0
	}

	/// Whether the node takes its parent's box entire, ignoring its own. On a tree's root this is
	/// what sizes a component to the instance using it.
	pub fn fill(&self) -> bool {
		self.0 & Self::FILL != 0
	}

	/// Which edges of the parent the node keeps a fixed distance from as the parent resizes. Both
	/// edges of an axis stretches the node along it; one edge moves the node with that edge.
	pub fn anchor_top(&self) -> bool {
		self.0 & Self::ANCHOR_TOP != 0
	}

	pub fn anchor_bottom(&self) -> bool {
		self.0 & Self::ANCHOR_BOTTOM != 0
	}

	pub fn anchor_left(&self) -> bool {
		self.0 & Self::ANCHOR_LEFT != 0
	}

	pub fn anchor_right(&self) -> bool {
		self.0 & Self::ANCHOR_RIGHT != 0
	}

	/// Whether the node has a collision region.
	pub fn has_collision(&self) -> bool {
		self.0 & Self::HAS_COLLISION != 0
	}
}

impl fmt::Debug for NodeFlags {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let names = [
			(Self::VISIBLE, "visible"),
			(Self::ENABLED, "enabled"),
			(Self::CLIP, "clip"),
			(Self::FILL, "fill"),
			(Self::ANCHOR_TOP, "anchor_top"),
			(Self::ANCHOR_BOTTOM, "anchor_bottom"),
			(Self::ANCHOR_LEFT, "anchor_left"),
			(Self::ANCHOR_RIGHT, "anchor_right"),
			(Self::HAS_COLLISION, "has_collision"),
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

/// Read `count` nodes starting at `at`, returning them and where they end.
pub(super) fn read_nodes(bytes: &[u8], at: usize, count: u32) -> Result<(Vec<Node>, usize)> {
	let mut nodes = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let node = structs::Node::read(&mut cursor(bytes, at)?)?;
		let size = usize::from(node.node_offset);
		let payload = region(bytes, at, size, structs::Node::SIZE)?;
		nodes.push(Node::new(node, payload));
		at += size;
	}
	Ok((nodes, at))
}

/// Roots and children of a flat node list, shared by widgets and components.
pub(super) fn roots(nodes: &[Node]) -> impl Iterator<Item = &Node> {
	nodes.iter().rev().filter(|node| node.parent_id == 0)
}

pub(super) fn children(nodes: &[Node], id: u32) -> impl Iterator<Item = &Node> {
	let parent = i32::try_from(id).unwrap_or(-1);
	nodes
		.iter()
		.rev()
		.filter(move |node| node.parent_id == parent)
}

#[cfg(test)]
mod test {
	use super::NodeFlags;

	#[test]
	fn flags_split_into_their_bits() {
		let anchors = |flags: NodeFlags| {
			(
				flags.anchor_top(),
				flags.anchor_bottom(),
				flags.anchor_left(),
				flags.anchor_right(),
			)
		};

		let static_text = NodeFlags(0x53);
		assert!(static_text.visible() && static_text.enabled());
		assert!(!static_text.fill() && !static_text.clip());
		assert_eq!(anchors(static_text), (true, false, true, false));

		assert_eq!(anchors(NodeFlags(0x93)), (true, false, false, true));
		assert_eq!(anchors(NodeFlags(0xD3)), (true, false, true, true));
		assert_eq!(anchors(NodeFlags(0xF3)), (true, true, true, true));

		let frame = NodeFlags(0x5B);
		assert!(frame.fill());
		assert!(!frame.anchor_right());
		assert_eq!(frame.bits(), 0x5B);

		// The collision bit rides on top of an otherwise ordinary word.
		let collision = NodeFlags(0x15B);
		assert!(collision.has_collision() && collision.fill());
		assert!(!NodeFlags(0x5B).has_collision());
		assert!(!NodeFlags(0x52).visible());
		assert_eq!(NodeFlags(0).bits(), 0);
	}
}
