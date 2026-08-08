use std::{fmt, io::Cursor};

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result, file::File};

use super::{
	component::{self, Component},
	cursor, invalid,
	node::{self, Node},
	region, structs,
	timeline::{self, Timeline},
};

/// A UI layout.
pub struct UiLayout {
	version: structs::Version,
	textures: Vec<Texture>,
	part_lists: Vec<PartList>,
	components: Vec<Component>,
	timelines: Vec<Timeline>,
	widgets: Vec<Widget>,
}

impl UiLayout {
	/// Version of the layout structure. Lists carry their own versions, which may differ.
	pub fn version(&self) -> structs::Version {
		self.version
	}

	/// Textures the layout draws from.
	pub fn textures(&self) -> &[Texture] {
		&self.textures
	}

	/// Sprite rectangles cut out of those textures.
	pub fn part_lists(&self) -> &[PartList] {
		&self.part_lists
	}

	/// Reusable pieces of interface the widgets instance.
	pub fn components(&self) -> &[Component] {
		&self.components
	}

	/// Animations the nodes reference.
	pub fn timelines(&self) -> &[Timeline] {
		&self.timelines
	}

	/// The screens this layout defines.
	pub fn widgets(&self) -> &[Widget] {
		&self.widgets
	}

	/// The texture with `id`, which is what a [`Part`] references.
	pub fn texture(&self, id: u32) -> Option<&Texture> {
		self.textures.iter().find(|texture| texture.id == id)
	}

	/// The part list with `id`, which is what an image or nine-grid node references.
	pub fn part_list(&self, id: u32) -> Option<&PartList> {
		self.part_lists.iter().find(|list| list.id == id)
	}

	/// The component with `id`, which is what an instancing node's type tag holds.
	pub fn component(&self, id: u32) -> Option<&Component> {
		self.components
			.iter()
			.find(|component| component.id() == id)
	}

	/// The timeline with `id`, as named by [`Node::timeline_id`].
	pub fn timeline(&self, id: u32) -> Option<&Timeline> {
		self.timelines.iter().find(|timeline| timeline.id() == id)
	}

	/// The widget with `id`.
	pub fn widget(&self, id: u32) -> Option<&Widget> {
		self.widgets.iter().find(|widget| widget.id == id)
	}
}

impl UiLayout {
	fn parse(bytes: &[u8]) -> Result<Self> {
		let header = structs::Header::read(&mut cursor(bytes, 0)?)?;

		// The two section headers are tables of contents; which one declares a given list is an
		// artefact of how the file was written, so they are read together and merged.
		let mut sections = Vec::new();
		for offset in [header.resource_section_offset, header.widget_section_offset] {
			let at = to_usize(offset);
			if offset == 0 || sections.iter().any(|(base, _)| *base == at) {
				continue;
			}
			sections.push((at, structs::Section::read(&mut cursor(bytes, at)?)?));
		}

		// A list offset is relative to the section that declares it, so the base travels with it.
		let locate = |pick: fn(&structs::Section) -> u32| {
			sections
				.iter()
				.find(|(_, section)| pick(section) != 0)
				.map(|(base, section)| base + to_usize(pick(section)))
		};

		let assets = list(
			bytes,
			locate(|s| s.asset_list_offset),
			structs::ListHeader::ASSETS,
		)?;
		let parts = list(
			bytes,
			locate(|s| s.part_list_offset),
			structs::ListHeader::PARTS,
		)?;
		let components = list(
			bytes,
			locate(|s| s.component_list_offset),
			structs::ListHeader::COMPONENTS,
		)?;
		let timelines = list(
			bytes,
			locate(|s| s.timeline_list_offset),
			structs::ListHeader::TIMELINES,
		)?;
		let widgets = list(
			bytes,
			locate(|s| s.widget_list_offset),
			structs::ListHeader::WIDGETS,
		)?;

		Ok(Self {
			version: header.version,
			textures: match assets {
				Some((head, at)) => read_textures(bytes, at, head.count, head.version)?,
				None => Vec::new(),
			},
			part_lists: match parts {
				Some((head, at)) => read_part_lists(bytes, at, head.count)?,
				None => Vec::new(),
			},
			components: match components {
				Some((head, at)) => component::read_components(bytes, at, head.count)?,
				None => Vec::new(),
			},
			timelines: match timelines {
				Some((head, at)) => timeline::read_timelines(bytes, at, head.count)?,
				None => Vec::new(),
			},
			widgets: match widgets {
				Some((head, at)) => read_widgets(bytes, at, head.count)?,
				None => Vec::new(),
			},
		})
	}
}

impl File for UiLayout {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		Self::parse(&bytes)
	}
}

impl fmt::Debug for UiLayout {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("UiLayout")
			.field("version", &self.version)
			.field("textures", &self.textures.len())
			.field("part_lists", &self.part_lists.len())
			.field("components", &self.components.len())
			.field("timelines", &self.timelines.len())
			.field("widgets", &self.widgets.len())
			.finish_non_exhaustive()
	}
}

/// A texture the layout draws from.
#[derive(Debug, Clone, CopyGetters)]
pub struct Texture {
	/// Identifier, which is what a [`Part`] references.
	#[get_copy = "pub"]
	id: u32,

	path: String,

	/// An icon to use in place of a path, when the path is empty.
	#[get_copy = "pub"]
	icon_id: u32,

	/// Which of the game's UI themes the texture has variants for. Absent before `ashd` 0101.
	#[get_copy = "pub"]
	theme_bitmask: Option<u8>,
}

impl Texture {
	/// Path to the texture.
	pub fn path(&self) -> &str {
		&self.path
	}
}

/// A set of sprites cut from the textures.
#[derive(Debug, Clone, CopyGetters)]
pub struct PartList {
	/// Identifier, which is what an image or nine-grid node references.
	#[get_copy = "pub"]
	id: u32,

	parts: Vec<Part>,
}

impl PartList {
	/// The sprites, in the order a node's part index addresses them.
	pub fn parts(&self) -> &[Part] {
		&self.parts
	}
}

/// A rectangle of a texture, used as a sprite.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Part {
	/// The texture this rectangle is cut from.
	texture_id: u32,
	/// Top-left corner within that texture, in pixels.
	u: u16,
	v: u16,
	/// Size in pixels.
	width: u16,
	height: u16,
}

/// A screen: a tree of nodes and where it sits.
#[derive(Debug, CopyGetters)]
pub struct Widget {
	#[get_copy = "pub"]
	id: u32,

	/// Which corner or edge of the screen the widget is positioned from.
	#[get_copy = "pub"]
	alignment: Alignment,

	/// Whether the textures this widget uses have per-theme variants.
	#[get_copy = "pub"]
	themed_assets: bool,

	/// Position from the aligned origin, in pixels.
	#[get_copy = "pub"]
	x: i16,
	#[get_copy = "pub"]
	y: i16,

	nodes: Vec<Node>,
}

impl Widget {
	/// The widget's nodes, in file order.
	pub fn nodes(&self) -> &[Node] {
		&self.nodes
	}

	/// The node with `id`, if this widget has one. Ids are scoped to their tree, so the same id in
	/// another widget or a component is a different node.
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
}

/// Where inside its parent an element is anchored.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
	TopLeft,
	Top,
	TopRight,
	Left,
	Center,
	Right,
	BottomLeft,
	Bottom,
	BottomRight,
	/// An alignment ironworks does not recognise; the inner value is the raw tag.
	Unknown(u8),
}

impl From<u8> for Alignment {
	fn from(value: u8) -> Self {
		match value {
			0x0 => Self::TopLeft,
			0x1 => Self::Top,
			0x2 => Self::TopRight,
			0x3 => Self::Left,
			0x4 => Self::Center,
			0x5 => Self::Right,
			0x6 => Self::BottomLeft,
			0x7 => Self::Bottom,
			0x8 => Self::BottomRight,
			other => Self::Unknown(other),
		}
	}
}

fn to_usize(value: u32) -> usize {
	usize::try_from(value).expect("u32 fits usize")
}

/// A list's header and where its first record starts, or `None` when the file declares no such
/// list. A list that is present but not the one expected means the offset was misread, so
/// everything downstream would be garbage.
fn list(
	bytes: &[u8],
	at: Option<usize>,
	magic: &[u8; 4],
) -> Result<Option<(structs::ListHeader, usize)>> {
	let Some(at) = at else {
		return Ok(None);
	};
	let header = structs::ListHeader::read(&mut cursor(bytes, at)?)?;
	if &header.magic != magic {
		return Err(invalid(format!(
			"expected {} at {at:#x}, found {}",
			String::from_utf8_lossy(magic),
			String::from_utf8_lossy(&header.magic),
		)));
	}
	Ok(Some((header, at + structs::ListHeader::SIZE)))
}

/// A fixed-width, NUL-padded string field.
fn string(bytes: &[u8]) -> String {
	let end = bytes
		.iter()
		.position(|byte| *byte == 0)
		.unwrap_or(bytes.len());
	String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn read_textures(
	bytes: &[u8],
	at: usize,
	count: u32,
	version: structs::Version,
) -> Result<Vec<Texture>> {
	// The asset list's own version sets the entry stride, so a version that is not a number cannot
	// be defaulted past -- picking the wrong stride desyncs every entry after the first.
	let number = version
		.number()
		.ok_or_else(|| invalid(format!("asset list version {version:?} is not a number")))?;
	let themed = number > 100;
	let stride = structs::TextureEntry::stride(themed);

	let mut textures = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let entry = structs::TextureEntry::read_args(&mut cursor(bytes, at)?, (themed,))?;
		textures.push(Texture {
			id: entry.id,
			path: string(&entry.path),
			icon_id: entry.icon_id,
			theme_bitmask: entry.theme_bitmask,
		});
		at += stride;
	}
	Ok(textures)
}

fn read_part_lists(bytes: &[u8], at: usize, count: u32) -> Result<Vec<PartList>> {
	let mut lists = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let head = structs::PartList::read(&mut cursor(bytes, at)?)?;
		let size = to_usize(head.offset);
		let payload = region(bytes, at, size, structs::PartList::SIZE)?;

		let mut reader = Cursor::new(payload);
		let parts = (0..head.part_count)
			.map(|_| Part::read(&mut reader))
			.collect::<binrw::BinResult<Vec<_>>>()?;

		lists.push(PartList { id: head.id, parts });
		at += size;
	}
	Ok(lists)
}

fn read_widgets(bytes: &[u8], at: usize, count: u32) -> Result<Vec<Widget>> {
	let mut widgets = Vec::new();
	let mut at = at;
	for _ in 0..count {
		let head = structs::Widget::read(&mut cursor(bytes, at)?)?;
		let size = usize::from(head.offset);
		region(bytes, at, size, structs::Widget::SIZE)?;
		let end = at + size;

		let (nodes, nodes_end) = node::read_nodes(
			bytes,
			at + structs::Widget::SIZE,
			u32::from(head.node_count),
		)?;
		if nodes_end > end {
			return Err(invalid(format!(
				"widget at {at:#x} has nodes running {} bytes past the record",
				nodes_end - end
			)));
		}

		widgets.push(Widget {
			id: head.id,
			alignment: Alignment::from(head.alignment),
			themed_assets: head.themed_assets != 0,
			x: head.x,
			y: head.y,
			nodes,
		});
		at = end;
	}
	Ok(widgets)
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use crate::{
		error::Error,
		file::File,
		file::uld::{KeyGroupKind, KeyUsage, Node, NodeKind},
	};

	use super::{UiLayout, structs};

	const HEADER: usize = 16;
	const SECTION: usize = 36;

	/// A file laying its one resource list in `slot` (0 asset, 1 part, 2 component, 3 timeline) and
	/// its widget list in the second section, which is where real files put it.
	fn assemble(slot: Option<usize>, list: &[u8], widgets: Option<&[u8]>) -> Vec<u8> {
		let widget_section_at = HEADER + SECTION + list.len();

		let mut out = Vec::new();
		out.extend(b"uldh0100");
		out.extend(u32::try_from(HEADER).unwrap().to_le_bytes());
		out.extend(u32::try_from(widget_section_at).unwrap().to_le_bytes());
		out.extend(section(slot.map(|slot| (slot, SECTION))));
		out.extend_from_slice(list);
		out.extend(section(widgets.map(|_| (4, SECTION))));
		out.extend_from_slice(widgets.unwrap_or_default());
		out
	}

	/// A section header declaring at most one list, at `offset` from the header's own start.
	fn section(list: Option<(usize, usize)>) -> Vec<u8> {
		let mut offsets = [0u32; 7];
		if let Some((slot, offset)) = list {
			offsets[slot] = u32::try_from(offset).unwrap();
		}
		let mut out = Vec::from(*b"atkh0100");
		out.extend(offsets.iter().flat_map(|offset| offset.to_le_bytes()));
		out
	}

	fn list(magic: &[u8; 4], version: &[u8; 4], count: u32, body: &[u8]) -> Vec<u8> {
		let mut out = Vec::from(*magic);
		out.extend_from_slice(version);
		out.extend(count.to_le_bytes());
		out.extend(0i32.to_le_bytes());
		out.extend_from_slice(body);
		out
	}

	/// A node whose declared size covers its payload, with everything after the type tag zeroed.
	fn node(id: u32, parent: i32, node_type: i32, payload: &[u8]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(id.to_le_bytes());
		out.extend(parent.to_le_bytes());
		out.extend([0u8; 12]); // sibling and child links
		out.extend(node_type.to_le_bytes());
		out.extend(
			u16::try_from(structs::Node::SIZE + payload.len())
				.unwrap()
				.to_le_bytes(),
		);
		out.resize(structs::Node::SIZE, 0);
		out.extend_from_slice(payload);
		out
	}

	fn widget(id: u32, nodes: &[Vec<u8>]) -> Vec<u8> {
		let body = nodes.concat();
		let mut out = Vec::new();
		out.extend(id.to_le_bytes());
		out.extend([0u8; 8]); // alignment, theming, padding, position
		out.extend(u16::try_from(nodes.len()).unwrap().to_le_bytes());
		out.extend(
			u16::try_from(structs::Widget::SIZE + body.len())
				.unwrap()
				.to_le_bytes(),
		);
		out.extend(body);
		out
	}

	fn widgets(list_body: &[Vec<u8>]) -> Vec<u8> {
		let count = u32::try_from(list_body.len()).unwrap();
		list(b"wdhd", b"0100", count, &list_body.concat())
	}

	fn read(bytes: Vec<u8>) -> crate::error::Result<UiLayout> {
		UiLayout::read(Cursor::new(bytes))
	}

	#[test]
	fn empty() {
		assert!(matches!(read(Vec::new()), Err(Error::Resource(_))));
	}

	#[test]
	fn missing_magic() {
		assert!(matches!(
			read(b"hello world".to_vec()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn absent_lists() {
		let layout = read(assemble(None, &[], None)).unwrap();
		assert!(layout.textures().is_empty());
		assert!(layout.part_lists().is_empty());
		assert!(layout.components().is_empty());
		assert!(layout.timelines().is_empty());
		assert!(layout.widgets().is_empty());
	}

	/// The property the whole parser is built on: a node type ironworks does not model must not
	/// shift the nodes after it. Physis reads payloads inline with no such step, which is why a
	/// text node desyncs the rest of the file for it.
	#[test]
	fn unknown_node_type_does_not_desync() {
		let layout = read(assemble(
			None,
			&[],
			Some(&widgets(&[widget(
				1,
				&[
					node(10, 0, 2, &[0; 12]),
					// Below the component range, so it is genuinely unmodelled rather than an
					// instance of component 99.
					node(20, 0, 99, &[0xAB; 40]),
					node(30, 0, 3, &[0; 24]),
				],
			)])),
		))
		.unwrap();

		let nodes = layout.widgets()[0].nodes();
		assert_eq!(nodes.len(), 3);
		assert_eq!(nodes[0].id(), 10);
		assert_eq!(nodes[2].id(), 30, "the node after the unknown one moved");

		let NodeKind::Unknown { node_type, data } = nodes[1].kind() else {
			panic!("expected an unknown payload, got {:?}", nodes[1].kind());
		};
		assert_eq!(*node_type, 99);
		assert_eq!(data.len(), 40, "the payload was not kept whole");
	}

	/// A node claiming to be smaller than its own prefix would leave the enclosing loop where it
	/// started. The test failing to terminate is as much a failure as the assert.
	#[test]
	fn node_smaller_than_its_prefix() {
		let mut stalled = node(1, 0, 1, &[]);
		stalled[24..26].copy_from_slice(&0u16.to_le_bytes());
		let file = assemble(None, &[], Some(&widgets(&[widget(1, &[stalled])])));
		assert!(matches!(read(file), Err(Error::Invalid(..))));
	}

	#[test]
	fn record_past_end_of_file() {
		let mut file = assemble(
			None,
			&[],
			Some(&widgets(&[widget(1, &[node(1, 0, 1, &[])])])),
		);
		file.truncate(file.len() - 8);
		assert!(matches!(read(file), Err(Error::Invalid(..))));
	}

	/// Node ids restart in every tree, so a lookup has to be scoped to the tree it was asked of.
	#[test]
	fn node_ids_are_scoped_to_their_tree() {
		let layout = read(assemble(
			None,
			&[],
			Some(&widgets(&[
				widget(1, &[node(1, 0, 1, &[]), node(2, 1, 1, &[])]),
				widget(2, &[node(1, 0, 3, &[0; 24])]),
			])),
		))
		.unwrap();

		assert_eq!(layout.widgets().len(), 2);
		assert!(matches!(
			layout.widgets()[0].node(1).unwrap().kind(),
			NodeKind::Res
		));
		assert!(matches!(
			layout.widgets()[1].node(1).unwrap().kind(),
			NodeKind::Text(_)
		));
		// Children come from each node's parent rather than the parent's own child link.
		assert_eq!(layout.widgets()[0].children(1).count(), 1);
		assert_eq!(layout.widgets()[1].children(1).count(), 0);
	}

	/// The game's sibling list runs backwards through the file, so the last child written is the
	/// first drawn. Getting this the wrong way round paints a window's background over its contents.
	#[test]
	fn siblings_come_back_in_draw_order() {
		let layout = read(assemble(
			None,
			&[],
			Some(&widgets(&[widget(
				1,
				&[
					node(1, 0, 1, &[]),
					node(2, 1, 1, &[]),
					node(3, 1, 1, &[]),
					node(4, 1, 1, &[]),
				],
			)])),
		))
		.unwrap();

		let widget = &layout.widgets()[0];
		let order = |ids: Vec<&Node>| ids.iter().map(|node| node.id()).collect::<Vec<_>>();
		assert_eq!(order(widget.children(1).collect()), [4, 3, 2]);
		assert_eq!(order(widget.roots().collect()), [1]);
	}

	fn texture_entry(id: u32, path: &str, icon_id: u32, theme: Option<u8>) -> Vec<u8> {
		let mut out = id.to_le_bytes().to_vec();
		out.extend(path.bytes());
		out.resize(4 + 44, 0);
		out.extend(icon_id.to_le_bytes());
		if let Some(theme) = theme {
			out.extend([theme, 0, 0, 0]);
		}
		out
	}

	/// The asset list's version sets the entry stride, so the *second* entry is what catches a
	/// wrong one.
	#[test]
	fn texture_entry_stride_follows_the_list_version() {
		for (version, theme) in [(b"0100", None), (b"0101", Some(0x7F))] {
			let mut body = texture_entry(1, "ui/uld/first.tex", 0, theme);
			body.extend(texture_entry(2, "ui/uld/second.tex", 42, theme));
			let layout = read(assemble(Some(0), &list(b"ashd", version, 2, &body), None)).unwrap();

			let textures = layout.textures();
			assert_eq!(textures.len(), 2, "version {version:?}");
			assert_eq!(textures[1].id(), 2, "version {version:?}");
			assert_eq!(
				textures[1].path(),
				"ui/uld/second.tex",
				"version {version:?}"
			);
			assert_eq!(textures[1].icon_id(), 42, "version {version:?}");
			assert_eq!(textures[0].theme_bitmask(), theme, "version {version:?}");
		}
	}

	/// Defaulting a version that is not a number would pick a stride and desync every entry after
	/// the first, which is worse than refusing the file.
	#[test]
	fn unparseable_asset_version_is_an_error() {
		let body = texture_entry(1, "ui/uld/first.tex", 0, None);
		let file = assemble(Some(0), &list(b"ashd", b"abcd", 1, &body), None);
		assert!(matches!(read(file), Err(Error::Invalid(..))));
	}

	/// A list that is present but not the expected one means the offset was misread, so anything
	/// parsed from it would be fiction.
	#[test]
	fn wrong_list_magic_is_an_error() {
		let file = assemble(Some(0), &list(b"tphd", b"0100", 0, &[]), None);
		assert!(matches!(read(file), Err(Error::Invalid(..))));
	}

	/// The nesting from a real file: a key group of twelve 20-byte keyframes is 248 bytes, the
	/// animation holding it 264, and the timeline holding that 276.
	///
	/// The two runs of records a timeline declares are its animations and its label sets, so one of
	/// each here has to come back from the accessor for its own run and not the other.
	#[test]
	fn timeline_records_nest_by_declared_size() {
		let record = |groups: Vec<u8>| {
			let mut record = Vec::new();
			record.extend(1u32.to_le_bytes()); // start
			record.extend(59u32.to_le_bytes()); // end
			record.extend(u32::try_from(16 + groups.len()).unwrap().to_le_bytes());
			record.extend(1u32.to_le_bytes()); // key groups
			record.extend(groups);
			record
		};

		let mut alpha = Vec::new();
		alpha.extend(3u16.to_le_bytes()); // usage: alpha
		alpha.extend(6u16.to_le_bytes()); // kind: byte1
		alpha.extend(248u16.to_le_bytes());
		alpha.extend(12u16.to_le_bytes());
		alpha.resize(248, 0);
		let animation = record(alpha);
		assert_eq!(animation.len(), 264);

		let mut label = Vec::new();
		label.extend(0u16.to_le_bytes()); // usage
		label.extend(0x19u16.to_le_bytes()); // kind: label
		label.extend(28u16.to_le_bytes());
		label.extend(1u16.to_le_bytes());
		label.resize(28, 0);
		let label_set = record(label);

		let mut timeline = Vec::new();
		timeline.extend(7u32.to_le_bytes()); // id
		timeline.extend(
			u32::try_from(12 + animation.len() + label_set.len())
				.unwrap()
				.to_le_bytes(),
		);
		timeline.extend(1u16.to_le_bytes()); // animations
		timeline.extend(1u16.to_le_bytes()); // label sets
		timeline.extend(animation);
		timeline.extend(label_set);
		assert_eq!(timeline.len(), 320);

		let layout = read(assemble(
			Some(3),
			&list(b"tlhd", b"0100", 1, &timeline),
			None,
		))
		.unwrap();

		let timeline = layout.timeline(7).expect("timeline 7");
		assert_eq!(timeline.animations().len(), 1);
		assert_eq!(timeline.label_sets().len(), 1);

		let group = &timeline.animations()[0].groups()[0];
		assert_eq!(group.usage(), KeyUsage::Alpha);
		assert_eq!(group.kind(), KeyGroupKind::Byte1);
		assert_eq!(group.data().len(), 240);
		assert_eq!(group.keyframe_size(), Some(20));

		assert_eq!(
			timeline.label_sets()[0].groups()[0].kind(),
			KeyGroupKind::Label
		);
	}

	/// Which section declares a list is an artefact of how the file was written, and each section's
	/// offsets are relative to itself -- so resolving one against the other's base would land on a
	/// plausible but wrong address.
	#[test]
	fn a_list_is_found_in_either_section() {
		let body = texture_entry(1, "ui/uld/first.tex", 0, None);
		let assets = list(b"ashd", b"0100", 1, &body);

		// The second section declares the assets; the first declares nothing.
		let mut file = Vec::new();
		file.extend(b"uldh0100");
		file.extend(u32::try_from(HEADER).unwrap().to_le_bytes());
		file.extend(u32::try_from(HEADER + SECTION).unwrap().to_le_bytes());
		file.extend(section(None));
		file.extend(section(Some((0, SECTION))));
		file.extend(assets);

		let layout = read(file).unwrap();
		assert_eq!(layout.textures().len(), 1);
		assert_eq!(layout.textures()[0].path(), "ui/uld/first.tex");
	}
}
