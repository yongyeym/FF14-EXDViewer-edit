use std::fmt;

use crate::{
	FileStream,
	error::Result,
	file::{
		File,
		shader::{Bands, DirectX, Resource, Walk, name, to_usize},
	},
};

use super::structs;

/// One compiled shader, as the game's post effect and compute passes load it.
/// Shader bytecode is not kept.
pub struct ShaderCode {
	version: u32,
	stage: Stage,
	directx: DirectX,
	blob_offset: usize,
	blob_size: usize,
	bands: Bands,
	resources: Vec<Resource>,
	strings: Vec<u8>,
}

impl ShaderCode {
	pub fn version(&self) -> u32 {
		self.version
	}

	/// Which pipeline stage the shader runs at.
	pub fn stage(&self) -> Stage {
		self.stage
	}

	/// Which DirectX the shader was compiled for.
	pub fn directx(&self) -> DirectX {
		self.directx
	}

	/// Where the bytecode sits in the file the shader was read from, and how long it is, which locate
	/// it in the original bytes.
	pub fn blob_offset(&self) -> usize {
		self.blob_offset
	}

	pub fn blob_size(&self) -> usize {
		self.blob_size
	}

	/// What the shader binds: constant buffers, then samplers, textures and unordered access views.
	pub fn resources(&self) -> &[Resource] {
		&self.resources
	}

	/// The constant buffers the shader binds.
	pub fn constants(&self) -> &[Resource] {
		self.bands.constants(&self.resources)
	}

	/// The samplers the shader binds.
	pub fn samplers(&self) -> &[Resource] {
		self.bands.samplers(&self.resources)
	}

	/// The textures the shader binds. Versions before `0x0601` list none, binding their textures
	/// through the samplers they share a name with.
	pub fn textures(&self) -> &[Resource] {
		self.bands.textures(&self.resources)
	}

	/// The unordered access views the shader binds.
	pub fn uavs(&self) -> &[Resource] {
		self.bands.uavs(&self.resources)
	}

	/// The name of a resource, or `None` where it points outside the string block.
	pub fn name(&self, resource: &Resource) -> Option<&str> {
		name(&self.strings, resource)
	}
}

impl ShaderCode {
	pub fn parse(bytes: &[u8]) -> Result<Self> {
		let mut walk = Walk::new("SHCD", bytes);
		let header = walk.read::<structs::Header>()?;

		let [low, mid, high] = header.version;
		let version = u32::from_le_bytes([low, mid, high, 0]);

		walk.declared_size(header.total_size)?;
		let (payload, strings_at) =
			walk.sections(header.blob_offset, header.strings_offset, "blob")?;

		let shader = walk.read_args::<structs::Shader, _>((version,))?;
		let bands = Bands::from(&shader.counts);
		let resources = walk.resources(bands.total(), "resource table")?;
		walk.ends_at(payload, "shader blob")?;

		let blob_size = to_usize(shader.blob_size);
		let blob_offset = payload
			.checked_add(to_usize(shader.blob_offset))
			.filter(|start| {
				start
					.checked_add(blob_size)
					.is_some_and(|end| end <= strings_at)
			})
			.ok_or_else(|| {
				walk.invalid(format!(
					"a blob of {blob_size} bytes does not fit between {payload:#x} and the string block at {strings_at:#x}"
				))
			})?;

		Ok(Self {
			version,
			stage: Stage::from(header.stage),
			directx: DirectX::from(header.directx),
			blob_offset,
			blob_size,
			bands,
			resources,
			strings: bytes[strings_at..].to_vec(),
		})
	}
}

impl File for ShaderCode {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		Self::parse(&bytes)
	}
}

impl fmt::Debug for ShaderCode {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ShaderCode")
			.field("version", &format_args!("{:#06x}", self.version))
			.field("stage", &self.stage)
			.field("directx", &self.directx)
			.field("resources", &self.resources.len())
			.field("blob_size", &self.blob_size)
			.finish_non_exhaustive()
	}
}

/// Which pipeline stage a shader runs at. The tags are the file's own, which do not follow the order
/// a .shpk lists its stages in.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
	Vertex,
	Pixel,
	Geometry,
	Compute,
	Hull,
	Domain,
	/// A tag ironworks does not recognise.
	Unknown(u8),
}

impl From<u8> for Stage {
	fn from(value: u8) -> Self {
		match value {
			0 => Self::Vertex,
			1 => Self::Pixel,
			2 => Self::Geometry,
			3 => Self::Compute,
			4 => Self::Hull,
			5 => Self::Domain,
			other => Self::Unknown(other),
		}
	}
}
