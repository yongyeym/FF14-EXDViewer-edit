use binrw::binread;
use getset::CopyGetters;

/// The width of the material mask a primitive carries, which nothing in the file encodes.
///
/// Every mesh the game still references writes the wide form, and the narrow files are orphaned
/// pre-repack assets. A mesh whose nodes each hold a single primitive reads the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialWidth {
	/// Eight bytes, for a primitive twelve bytes long.
	Wide,

	/// Two bytes, for a primitive six bytes long.
	Narrow,
}

/// An axis-aligned bounding box.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct BoundingBox {
	min: [f32; 3],
	max: [f32; 3],
}

/// One triangle of a mesh.
#[binread]
#[br(little, import(width: MaterialWidth))]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Primitive {
	/// The triangle's corners, as positions in the node's vertices.
	indices: [u8; 3],

	/// Both references read this byte as padding, but it carries a value in a small number of
	/// primitives.
	unknown_a: u8,

	#[br(temp, if(width == MaterialWidth::Narrow, 0))]
	narrow_material: u16,

	#[br(temp, if(width == MaterialWidth::Wide, 0))]
	wide_material: u64,

	/// Mask the game filters collision against, so that a surface can be solid to some things and
	/// not others.
	#[br(calc = match width {
		MaterialWidth::Narrow => u64::from(narrow_material),
		MaterialWidth::Wide => wide_material,
	})]
	material: u64,
}

#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Header {
	/// Zero for a mesh. A `list.pcb` writes its entry count here, which is how the two formats
	/// sharing the extension are told apart.
	pub kind: u32,

	pub version: u32,

	/// Every node of the tree but the root.
	pub total_child_nodes: u32,

	pub _total_primitives: u32,
}

impl Header {
	pub const SIZE: usize = 0x10;
}

#[binread]
#[br(little, import(width: MaterialWidth))]
#[derive(Debug)]
pub struct Node {
	pub unknown_a: u64,

	/// Byte offsets from this node's own start, zero where there is no child.
	pub child_offsets: [i32; 2],

	pub bounds: BoundingBox,

	#[br(temp)]
	compressed_count: u16,

	#[br(temp)]
	primitive_count: u16,

	#[br(temp, pad_after = 2)]
	raw_count: u16,

	#[br(count = raw_count)]
	pub raw_vertices: Vec<[f32; 3]>,

	#[br(count = compressed_count)]
	pub compressed_vertices: Vec<[u16; 3]>,

	#[br(count = primitive_count, args { inner: (width,) })]
	pub primitives: Vec<Primitive>,
}
