use binrw::BinRead;
use derivative::Derivative;
use getset::CopyGetters;

use crate::error::Result;

use super::{
	cursor, invalid,
	node::{self, Node},
	region, structs,
};

#[derive(Derivative, CopyGetters)]
#[derivative(Debug)]
pub struct Component {
	#[get_copy = "pub"]
	id: u32,

	#[get_copy = "pub"]
	kind: ComponentKind,

	#[get_copy = "pub"]
	ignore_input: bool,

	#[get_copy = "pub"]
	drag_arrow: bool,

	#[get_copy = "pub"]
	drop_arrow: bool,

	nodes: Vec<Node>,

	#[derivative(Debug = "ignore")]
	trailing: Vec<u8>,
}

impl Component {
	pub fn nodes(&self) -> &[Node] {
		&self.nodes
	}

	pub fn node(&self, id: u32) -> Option<&Node> {
		self.nodes.iter().find(|node| node.id() == id)
	}

	/// Nodes with no parent, in draw order.
	pub fn roots(&self) -> impl Iterator<Item = &Node> {
		node::roots(&self.nodes)
	}

	/// Direct children of `id`, in draw order: the first drawn, and so the furthest back, comes first.
	pub fn children(&self, id: u32) -> impl Iterator<Item = &Node> {
		node::children(&self.nodes, id)
	}

	/// The kind-specific bytes between the component's header and its node list. ironworks does not
	/// model these; they are mostly ids of nodes the component treats specially.
	pub fn trailing(&self) -> &[u8] {
		&self.trailing
	}
}

/// What a component is, which decides how the game drives it.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
	Base,
	Button,
	Window,
	CheckBox,
	RadioButton,
	GaugeBar,
	Slider,
	TextInput,
	NumericInput,
	List,
	DropDownList,
	Tab,
	TreeList,
	ScrollBar,
	ListItemRenderer,
	Icon,
	IconText,
	DragDrop,
	GuildLeveCard,
	TextNineGrid,
	JournalCanvas,
	Multipurpose,
	Map,
	Preview,
	HoldButton,
	Portrait,
	XbmItem,
	XbmContentStageEventMap,
	/// A kind ironworks does not recognise; the inner value is the raw tag.
	Unknown(u8),
}

impl From<u8> for ComponentKind {
	fn from(value: u8) -> Self {
		match value {
			0x00 => Self::Base,
			0x01 => Self::Button,
			0x02 => Self::Window,
			0x03 => Self::CheckBox,
			0x04 => Self::RadioButton,
			0x05 => Self::GaugeBar,
			0x06 => Self::Slider,
			0x07 => Self::TextInput,
			0x08 => Self::NumericInput,
			0x09 => Self::List,
			0x0A => Self::DropDownList,
			0x0B => Self::Tab,
			0x0C => Self::TreeList,
			0x0D => Self::ScrollBar,
			0x0E => Self::ListItemRenderer,
			0x0F => Self::Icon,
			0x10 => Self::IconText,
			0x11 => Self::DragDrop,
			0x12 => Self::GuildLeveCard,
			0x13 => Self::TextNineGrid,
			0x14 => Self::JournalCanvas,
			0x15 => Self::Multipurpose,
			0x16 => Self::Map,
			0x17 => Self::Preview,
			0x18 => Self::HoldButton,
			0x19 => Self::Portrait,
			0x1A => Self::XbmItem,
			0x1B => Self::XbmContentStageEventMap,
			other => Self::Unknown(other),
		}
	}
}

pub(super) fn read_components(bytes: &[u8], at: usize, count: u32) -> Result<Vec<Component>> {
	let mut components = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let head = structs::Component::read(&mut cursor(bytes, at)?)?;
		let size = usize::from(head.size);
		// Unlike everything else, a component's node list is bounded twice: by where the record says
		// it starts, and by the record's own total size.
		region(bytes, at, size, structs::Component::SIZE)?;
		let end = at + size;
		let nodes_at = at + usize::from(head.offset);
		if nodes_at < at + structs::Component::SIZE || nodes_at > end {
			return Err(invalid(format!(
				"component at {at:#x} starts its nodes at {:#x}, outside the record",
				head.offset
			)));
		}

		let (nodes, nodes_end) = node::read_nodes(bytes, nodes_at, head.node_count)?;
		if nodes_end > end {
			return Err(invalid(format!(
				"component at {at:#x} has nodes running {} bytes past the record",
				nodes_end - end
			)));
		}

		components.push(Component {
			id: head.id,
			kind: ComponentKind::from(head.kind),
			ignore_input: head.ignore_input != 0,
			drag_arrow: head.drag_arrow != 0,
			drop_arrow: head.drop_arrow != 0,
			nodes,
			trailing: bytes[at + structs::Component::SIZE..nodes_at].to_vec(),
		});
		at = end;
	}
	Ok(components)
}
