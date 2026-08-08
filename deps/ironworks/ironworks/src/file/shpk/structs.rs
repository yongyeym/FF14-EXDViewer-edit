use binrw::binread;

use crate::file::shader::Counts;

/// Versions from this one on carry hull, domain and geometry shaders, a word per shader entry, two
/// per node, and three shader indices per pass.
pub const VERSION_TESSELLATION: u32 = 0x0D01;

/// Versions from this one on carry node alias clusters.
pub const VERSION_NODE_ALIAS_CLUSTERS: u32 = 0x0E01;

/// The file's root header. Its size varies with the version, so the walk takes it from where the
/// read finished rather than a constant.
#[binread]
#[br(little, magic = b"ShPk")]
#[derive(Debug)]
pub struct Header {
	pub version: u32,
	pub directx: [u8; 4],

	pub total_size: u32,

	pub blobs_offset: u32,
	pub strings_offset: u32,

	pub vertex_count: u32,
	pub pixel_count: u32,

	/// Bytes the material parameter buffer takes, and the length of the defaults blob when one is
	/// present.
	pub material_params_size: u32,
	pub material_param_count: u16,
	pub has_param_defaults: u16,

	pub constant_count: u32,
	pub sampler_count: u16,
	pub texture_count: u16,
	pub uav_count: u32,

	pub system_key_count: u32,
	pub scene_key_count: u32,
	pub material_key_count: u32,

	pub node_count: u32,
	pub node_alias_count: u32,

	#[br(if(version >= VERSION_TESSELLATION))]
	pub hull_count: u32,
	#[br(if(version >= VERSION_TESSELLATION))]
	pub domain_count: u32,
	#[br(if(version >= VERSION_TESSELLATION))]
	pub geometry_count: u32,

	#[br(if(version >= VERSION_NODE_ALIAS_CLUSTERS))]
	pub node_alias_cluster_count: u32,
}

/// The fixed part of a shader entry, followed by its resource bindings.
#[binread]
#[br(little, import(version: u32))]
#[derive(Debug)]
pub struct Shader {
	pub blob_offset: u32,
	pub blob_size: u32,

	#[br(args(true))]
	pub counts: Counts,

	#[br(if(version >= VERSION_TESSELLATION))]
	_unknown: u32,
}

/// One value in the material parameter buffer.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
pub struct MaterialParam {
	pub id: u32,
	pub byte_offset: u16,
	pub byte_size: u16,
}

impl MaterialParam {
	pub const SIZE: usize = 8;
}

/// A switch selecting between shader variants.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
pub struct Key {
	pub id: u32,
	pub default_value: u32,
}

impl Key {
	pub const SIZE: usize = 8;
}

/// A selector standing in for a node.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy)]
pub struct Alias {
	pub selector: u32,
	pub node: u32,
}

impl Alias {
	pub const SIZE: usize = 8;
}
