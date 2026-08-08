use getset::CopyGetters;
use half::f16;

use crate::error::Result;

use super::{block::Block, invalid};

/// Geometry a particle or emitter draws from, held whole in the file rather than named as a
/// `.mdl` beside it.
#[derive(Debug)]
pub struct Model {
	vertices: Vec<Vertex>,
	triangles: Vec<Triangle>,
	emit_vertices: Vec<EmitVertex>,
	emit_vertex_numbers: Vec<u16>,
}

impl Model {
	/// The vertices the model draws, indexed by [`triangles`](Self::triangles).
	pub fn vertices(&self) -> &[Vertex] {
		&self.vertices
	}

	/// The triangles the model draws.
	pub fn triangles(&self) -> &[Triangle] {
		&self.triangles
	}

	/// The points an emitter can spawn particles at, which are separate from the drawn geometry.
	pub fn emit_vertices(&self) -> &[EmitVertex] {
		&self.emit_vertices
	}

	/// One number per [emit vertex](Self::emit_vertices), whose meaning has not been identified.
	pub fn emit_vertex_numbers(&self) -> &[u16] {
		&self.emit_vertex_numbers
	}

	pub(super) fn parse(blocks: &[Block]) -> Result<Self> {
		let array = |name: &str, stride: usize| match super::block::find(blocks, name) {
			Some(block) => {
				let bytes = block.bytes();
				match bytes.len() % stride {
					0 => Ok(bytes.chunks_exact(stride).collect()),
					_ => Err(invalid(format!(
						"{name} of {} bytes does not divide into records",
						bytes.len()
					))),
				}
			}
			None => Ok(Vec::new()),
		};

		Ok(Self {
			vertices: array("VDrw", Vertex::SIZE)?
				.into_iter()
				.map(Vertex::parse)
				.collect(),
			triangles: array("VIdx", Triangle::SIZE)?
				.into_iter()
				.map(Triangle::parse)
				.collect(),
			emit_vertices: array("VEmt", EmitVertex::SIZE)?
				.into_iter()
				.map(EmitVertex::parse)
				.collect(),
			emit_vertex_numbers: array("VNum", 2)?
				.into_iter()
				.map(|raw| u16::from_le_bytes(raw.try_into().unwrap()))
				.collect(),
		})
	}
}

/// One vertex of a model's drawn geometry.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Vertex {
	position: [f32; 4],

	/// Written as bytes biased by 128, so the zero direction is stored as `0x80`.
	normal: [i8; 4],

	/// Written the same way as [`normal`](Self::normal).
	tangent: [i8; 4],

	/// RGBA, one byte a channel.
	colour: [u8; 4],

	/// The four texture coordinate pairs the vertex carries.
	uv: [[f32; 2]; 4],
}

impl Vertex {
	const SIZE: usize = 36;

	fn parse(bytes: &[u8]) -> Self {
		let half = |at: usize| f16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()).to_f32();
		let signed =
			|at: usize| std::array::from_fn(|index| bytes[at + index].wrapping_sub(128) as i8);

		Self {
			position: std::array::from_fn(|index| half(index * 2)),
			normal: signed(8),
			tangent: signed(12),
			colour: bytes[16..20].try_into().unwrap(),
			uv: std::array::from_fn(|pair| {
				std::array::from_fn(|axis| half(20 + pair * 4 + axis * 2))
			}),
		}
	}
}

/// One point an emitter can spawn particles at.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct EmitVertex {
	position: [f32; 3],

	normal: [f32; 3],

	/// RGBA, one byte a channel.
	colour: [u8; 4],
}

impl EmitVertex {
	const SIZE: usize = 28;

	fn parse(bytes: &[u8]) -> Self {
		let float = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
		Self {
			position: std::array::from_fn(|index| float(index * 4)),
			normal: std::array::from_fn(|index| float(12 + index * 4)),
			colour: bytes[24..28].try_into().unwrap(),
		}
	}
}

/// Three indices into a model's [vertices](Model::vertices).
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Triangle {
	indices: [u16; 3],
}

impl Triangle {
	const SIZE: usize = 6;

	fn parse(bytes: &[u8]) -> Self {
		Self {
			indices: std::array::from_fn(|index| {
				u16::from_le_bytes(bytes[index * 2..index * 2 + 2].try_into().unwrap())
			}),
		}
	}
}
