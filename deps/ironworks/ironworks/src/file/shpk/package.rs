use std::fmt;

use getset::CopyGetters;

use crate::{
	FileStream,
	error::Result,
	file::{
		File,
		shader::{Bands, DirectX, Resource, Walk, name, to_usize},
	},
};

use super::structs;

/// A package of compiled shaders, as named by a material.
/// Shader bytecode is not kept.
pub struct ShaderPackage {
	version: u32,
	directx: DirectX,
	shaders: Vec<Shader>,
	param_buffer_size: u32,
	material_params: Vec<MaterialParam>,
	param_defaults: Vec<f32>,
	constants: Vec<Resource>,
	samplers: Vec<Resource>,
	textures: Vec<Resource>,
	uavs: Vec<Resource>,
	system_keys: Vec<Key>,
	scene_keys: Vec<Key>,
	material_keys: Vec<Key>,
	subview_defaults: [u32; 2],
	nodes: Vec<Node>,
	aliases: Vec<NodeAlias>,
	clusters: Vec<AliasCluster>,
	blobs_offset: usize,
	bytecode_size: usize,
	strings: Vec<u8>,
}

/// A shader stage a pass does not use.
pub const NONE: u32 = u32::MAX;

/// One combination of key values, and the shaders it selects.
///
/// This is a variant: the values it holds are the conditions its shaders were compiled under, so
/// reading them against the package's key list says what a shader is for rather than merely which
/// number it has.
#[derive(Debug)]
pub struct Node {
	id: u32,
	keys: Vec<u32>,
	passes: Vec<Pass>,
}

impl Node {
	pub fn id(&self) -> u32 {
		self.id
	}

	/// A value for each of the package's keys, in the order it declares them: system, then scene,
	/// then material, then the two subview keys.
	pub fn keys(&self) -> &[u32] {
		&self.keys
	}

	pub fn passes(&self) -> &[Pass] {
		&self.passes
	}
}

/// One pass of a variant, and the shader each stage runs for it. A stage the pass does not use is
/// [`NONE`], as are the three later stages on a package that predates them.
#[derive(Debug, Clone, Copy)]
pub struct Pass {
	id: u32,
	vertex: u32,
	pixel: u32,
	geometry: u32,
	hull: u32,
	domain: u32,
}

impl Pass {
	pub fn id(&self) -> u32 {
		self.id
	}

	/// The shader each stage runs, in the order the package stores its own shader lists.
	pub fn stages(&self) -> [u32; 5] {
		[
			self.vertex,
			self.pixel,
			self.hull,
			self.domain,
			self.geometry,
		]
	}

	pub fn vertex(&self) -> u32 {
		self.vertex
	}

	pub fn pixel(&self) -> u32 {
		self.pixel
	}
}

/// A selector that resolves to a node rather than carrying its own key values.
#[derive(Debug, Clone, Copy)]
pub struct NodeAlias {
	selector: u32,
	node: u32,
}

impl NodeAlias {
	pub fn selector(&self) -> u32 {
		self.selector
	}

	/// Which node the selector stands for.
	pub fn node(&self) -> u32 {
		self.node
	}
}

/// A grouping of aliases. Not sure what this is for.
#[derive(Debug)]
pub struct AliasCluster {
	id: u32,
	subclusters: Vec<SubCluster>,
}

impl AliasCluster {
	pub fn id(&self) -> u32 {
		self.id
	}

	pub fn subclusters(&self) -> &[SubCluster] {
		&self.subclusters
	}
}

#[derive(Debug)]
pub struct SubCluster {
	index: u16,
	alias_count: u16,
	data: Vec<u32>,
}

impl SubCluster {
	pub fn index(&self) -> u16 {
		self.index
	}

	pub fn alias_count(&self) -> u16 {
		self.alias_count
	}

	pub fn data(&self) -> &[u32] {
		&self.data
	}
}

impl ShaderPackage {
	pub fn version(&self) -> u32 {
		self.version
	}

	/// Which DirectX the shaders were compiled for.
	pub fn directx(&self) -> DirectX {
		self.directx
	}

	/// Every shader, vertex stage first, in the order the file lists them.
	pub fn shaders(&self) -> &[Shader] {
		&self.shaders
	}

	/// Bytes the material parameter buffer takes, which the parameters below carve up.
	pub fn param_buffer_size(&self) -> u32 {
		self.param_buffer_size
	}

	/// The layout of the material parameter buffer. A material's constants name these by id.
	pub fn material_params(&self) -> &[MaterialParam] {
		&self.material_params
	}

	/// The shader's defaults for the whole parameter buffer, or empty where it carries none.
	/// Indexed by float, so a parameter's byte offset over four indexes into it.
	pub fn param_defaults(&self) -> &[f32] {
		&self.param_defaults
	}

	/// Constant buffers the package binds.
	pub fn constants(&self) -> &[Resource] {
		&self.constants
	}

	/// Samplers the package binds.
	pub fn samplers(&self) -> &[Resource] {
		&self.samplers
	}

	/// Textures the package binds.
	pub fn textures(&self) -> &[Resource] {
		&self.textures
	}

	/// Unordered access views the package binds.
	pub fn uavs(&self) -> &[Resource] {
		&self.uavs
	}

	/// Keys the game sets from engine state.
	pub fn system_keys(&self) -> &[Key] {
		&self.system_keys
	}

	/// Keys the game sets per scene.
	pub fn scene_keys(&self) -> &[Key] {
		&self.scene_keys
	}

	/// Keys a material sets, which is what a material's shader keys select against.
	pub fn material_keys(&self) -> &[Key] {
		&self.material_keys
	}

	/// Defaults for the two subview keys.
	pub fn subview_defaults(&self) -> [u32; 2] {
		self.subview_defaults
	}

	/// Every combination of key values the package knows, and the shaders each one selects. This is
	/// what turns a shader from a number into the variant it is: the values a node holds are the
	/// conditions the source was compiled under.
	pub fn nodes(&self) -> &[Node] {
		&self.nodes
	}

	/// Selectors standing in for a node, where a combination resolves to one already listed.
	pub fn aliases(&self) -> &[NodeAlias] {
		&self.aliases
	}

	pub fn clusters(&self) -> &[AliasCluster] {
		&self.clusters
	}

	/// Where the blob section begins in the file the package was read from. A shader's
	/// [`blob_offset`](Shader::blob_offset) is relative to this, so the two locate its bytecode in
	/// the original bytes.
	pub fn blobs_offset(&self) -> usize {
		self.blobs_offset
	}

	/// Bytes of compiled bytecode the file carries.
	pub fn bytecode_size(&self) -> usize {
		self.bytecode_size
	}

	/// The name of a resource, or `None` where it points outside the string block.
	pub fn name(&self, resource: &Resource) -> Option<&str> {
		name(&self.strings, resource)
	}

	/// The shader's default for a material parameter, or `None` where the package carries no
	/// defaults or the parameter points outside the buffer.
	pub fn param_default(&self, param: &MaterialParam) -> Option<&[f32]> {
		let start = usize::from(param.byte_offset) / 4;
		let len = usize::from(param.byte_size) / 4;
		self.param_defaults.get(start..start.checked_add(len)?)
	}
}

impl ShaderPackage {
	pub fn parse(bytes: &[u8]) -> Result<Self> {
		let mut walk = Walk::new("SHPK", bytes);
		let header = walk.read::<structs::Header>()?;

		walk.declared_size(header.total_size)?;
		let (blobs, strings_at) =
			walk.sections(header.blobs_offset, header.strings_offset, "blob section")?;

		let mut shaders = Vec::new();
		for (stage, count) in [
			(Stage::Vertex, header.vertex_count),
			(Stage::Pixel, header.pixel_count),
			(Stage::Hull, header.hull_count),
			(Stage::Domain, header.domain_count),
			(Stage::Geometry, header.geometry_count),
		] {
			for _ in 0..count {
				let shader = walk.read_args::<structs::Shader, _>((header.version,))?;
				let bands = Bands::from(&shader.counts);
				shaders.push(Shader {
					stage,
					blob_offset: shader.blob_offset,
					blob_size: shader.blob_size,
					resources: walk.resources(bands.total(), "shader resource table")?,
					bands,
				});
			}
		}

		let material_params = walk.table::<structs::MaterialParam>(
			usize::from(header.material_param_count),
			structs::MaterialParam::SIZE,
			"material parameter table",
		)?;

		let param_defaults = match header.has_param_defaults {
			0 => Vec::new(),
			_ => {
				let size = to_usize(header.material_params_size);
				let end = walk.extent(1, size, "material parameter defaults")?;
				let values = bytes[walk.at..end]
					.chunks_exact(4)
					.map(|word| f32::from_le_bytes(word.try_into().expect("chunk is four bytes")))
					.collect();
				walk.at = end;
				values
			}
		};

		let constants = walk.resources(to_usize(header.constant_count), "constant table")?;
		let samplers = walk.resources(usize::from(header.sampler_count), "sampler table")?;
		let textures = walk.resources(usize::from(header.texture_count), "texture table")?;
		let uavs = walk.resources(to_usize(header.uav_count), "unordered access view table")?;

		let system_keys = keys(&mut walk, header.system_key_count, "system key table")?;
		let scene_keys = keys(&mut walk, header.scene_key_count, "scene key table")?;
		let material_keys = keys(&mut walk, header.material_key_count, "material key table")?;

		let end = walk.extent(2, 4, "subview key defaults")?;
		let subview_defaults = [word(bytes, walk.at), word(bytes, walk.at + 4)];
		walk.at = end;

		let (nodes, aliases, clusters) = read_selectors(&mut walk, &header)?;

		walk.ends_at(blobs, "blob section")?;

		Ok(Self {
			version: header.version,
			directx: DirectX::from(header.directx),
			shaders,
			param_buffer_size: header.material_params_size,
			material_params: material_params
				.into_iter()
				.map(|param| MaterialParam {
					id: param.id,
					byte_offset: param.byte_offset,
					byte_size: param.byte_size,
				})
				.collect(),
			param_defaults,
			constants,
			samplers,
			textures,
			uavs,
			system_keys,
			scene_keys,
			material_keys,
			subview_defaults,
			nodes,
			aliases,
			clusters,
			blobs_offset: blobs,
			bytecode_size: strings_at - blobs,
			strings: bytes[strings_at..].to_vec(),
		})
	}
}

impl File for ShaderPackage {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		Self::parse(&bytes)
	}
}

impl fmt::Debug for ShaderPackage {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ShaderPackage")
			.field("version", &format_args!("{:#06x}", self.version))
			.field("directx", &self.directx)
			.field("shaders", &self.shaders.len())
			.field("material_params", &self.material_params.len())
			.field("constants", &self.constants.len())
			.field("samplers", &self.samplers.len())
			.field("textures", &self.textures.len())
			.field("bytecode_size", &self.bytecode_size)
			.finish_non_exhaustive()
	}
}

/// Which pipeline stage a shader runs at.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
	Vertex,
	Pixel,
	Hull,
	Domain,
	Geometry,
}

/// One compiled shader.
#[derive(Debug, Clone, CopyGetters)]
pub struct Shader {
	#[get_copy = "pub"]
	stage: Stage,

	/// Where the bytecode sits within the blob section, and how long it is. A vertex shader's blob
	/// opens with an extra header, 4 bytes under DX9 and 8 bytes under DX11.
	#[get_copy = "pub"]
	blob_offset: u32,
	#[get_copy = "pub"]
	blob_size: u32,

	bands: Bands,
	resources: Vec<Resource>,
}

impl Shader {
	/// What this shader binds: constant buffers, then samplers, textures and unordered access views.
	pub fn resources(&self) -> &[Resource] {
		&self.resources
	}

	/// The constant buffers this shader binds.
	pub fn constants(&self) -> &[Resource] {
		self.bands.constants(&self.resources)
	}

	/// The samplers this shader binds.
	pub fn samplers(&self) -> &[Resource] {
		self.bands.samplers(&self.resources)
	}

	/// The textures this shader binds.
	pub fn textures(&self) -> &[Resource] {
		self.bands.textures(&self.resources)
	}

	/// The unordered access views this shader binds.
	pub fn uavs(&self) -> &[Resource] {
		self.bands.uavs(&self.resources)
	}
}

/// One value in the material parameter buffer.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct MaterialParam {
	/// The crc32 of the param's name. A material's constants use the same ids.
	id: u32,

	/// Byte offset into the parameter buffer, and the length taken from it. 12 is a vec3.
	byte_offset: u16,
	byte_size: u16,
}

/// A switch selecting between shader variants.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Key {
	/// The crc32 of the key's name.
	id: u32,

	/// The value used where nothing sets the key.
	default_value: u32,
}

fn keys(walk: &mut Walk<'_>, count: u32, what: &str) -> Result<Vec<Key>> {
	Ok(walk
		.table::<structs::Key>(to_usize(count), structs::Key::SIZE, what)?
		.into_iter()
		.map(|record| Key {
			id: record.id,
			default_value: record.default_value,
		})
		.collect())
}

/// Read the node, alias and cluster tables.
fn read_selectors(
	walk: &mut Walk<'_>,
	header: &structs::Header,
) -> Result<(Vec<Node>, Vec<NodeAlias>, Vec<AliasCluster>)> {
	let bytes = walk.bytes;
	let tessellated = header.version >= structs::VERSION_TESSELLATION;
	// The later generation widened the slot table and gave a pass a shader for each of the three
	// stages it gained.
	let node_prefix = if tessellated { 32 } else { 24 };
	let pass_words = if tessellated { 6 } else { 3 };

	// A node carries a value for every key the package declares, plus the two subview keys.
	let key_count = to_usize(header.system_key_count)
		+ to_usize(header.scene_key_count)
		+ to_usize(header.material_key_count)
		+ 2;

	let mut nodes = Vec::with_capacity(to_usize(header.node_count));
	for _ in 0..header.node_count {
		let at = walk.at;
		let head = walk.extent(1, node_prefix + key_count * 4, "node")?;
		let count = to_usize(word(bytes, at + 4));
		let end = walk.extent_at(head, count, pass_words * 4, "node pass table")?;

		let keys = (0..key_count)
			.map(|index| word(bytes, at + node_prefix + index * 4))
			.collect();
		let passes = (0..count)
			.map(|index| {
				let pass = head + index * pass_words * 4;
				let stage = |step: usize| match step < pass_words {
					true => word(bytes, pass + step * 4),
					false => NONE,
				};
				Pass {
					id: stage(0),
					vertex: stage(1),
					pixel: stage(2),
					geometry: stage(3),
					hull: stage(4),
					domain: stage(5),
				}
			})
			.collect();
		nodes.push(Node {
			id: word(bytes, at),
			keys,
			passes,
		});
		walk.at = end;
	}

	let aliases = walk
		.table::<structs::Alias>(
			to_usize(header.node_alias_count),
			structs::Alias::SIZE,
			"node alias table",
		)?
		.into_iter()
		.map(|record| NodeAlias {
			selector: record.selector,
			node: record.node,
		})
		.collect();

	let mut clusters = Vec::with_capacity(to_usize(header.node_alias_cluster_count));
	for _ in 0..header.node_alias_cluster_count {
		const CLUSTER: usize = 16;
		const SUB_CLUSTER: usize = 392;
		let at = walk.at;
		let head = walk.extent(1, CLUSTER, "node alias cluster")?;
		// The sub-cluster count is the third word of the header; the fourth is unidentified. Taking
		// the wrong one reads a hash as a count.
		let count = to_usize(word(bytes, at + 8));
		let end = walk.extent_at(head, count, SUB_CLUSTER, "node alias sub-cluster table")?;
		let subclusters = (0..count)
			.map(|index| {
				let sub = head + index * SUB_CLUSTER;
				SubCluster {
					index: half(bytes, sub),
					alias_count: half(bytes, sub + 2),
					data: (0..97)
						.map(|word_at| word(bytes, sub + 4 + word_at * 4))
						.collect(),
				}
			})
			.collect();
		clusters.push(AliasCluster {
			id: word(bytes, at),
			subclusters,
		});
		walk.at = end;
	}

	Ok((nodes, aliases, clusters))
}

/// The half-word at `at`, which every caller has already bounds-checked.
fn half(bytes: &[u8], at: usize) -> u16 {
	u16::from_le_bytes(bytes[at..at + 2].try_into().expect("two bytes"))
}

/// The word at `at`, which every caller has already bounds-checked.
fn word(bytes: &[u8], at: usize) -> u32 {
	u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}
