use std::{
	io::{Cursor, Read, Seek, SeekFrom},
	sync::Arc,
};

use binrw::{BinRead, NullString, VecArgs};
use half::f16;

use crate::error::{Error, ErrorValue, Result};

use super::{
	model::{Lod, MeshKind},
	structs,
};

// TODO: improve the debug output of these things
/// A single mesh within a model.
#[derive(Debug)]
pub struct Mesh {
	pub(super) file: Arc<structs::File>,

	pub(super) level: Lod,
	pub(super) mesh_index: usize,
	pub(super) kinds: Vec<MeshKind>,
}

impl Mesh {
	// TODO: bones

	/// What the model draws this mesh for. A mesh listed in more than one of the lod's ranges
	/// carries every kind that names it.
	pub fn kinds(&self) -> &[MeshKind] {
		&self.kinds
	}

	/// The runs of [`indices`](Self::indices) the mesh is split into, in the order it lists them.
	pub fn submeshes(&self) -> Vec<Submesh> {
		let mesh = &self.file.meshes[self.mesh_index];
		let first = usize::from(mesh.sub_mesh_index);
		self.file.submeshes[first..first + usize::from(mesh.sub_mesh_count)]
			.iter()
			.map(|submesh| Submesh {
				start: (submesh.index_offset - mesh.start_index) as usize,
				count: submesh.index_count as usize,
				attributes: submesh.attribute_index_mask,
			})
			.collect()
	}

	// TODO: i'm not sure this should be specific to mesh - the list of materials on the model might be useful in some cases. should i use a ref to the parent model and read off that, rather than the arc of a file?
	/// Path to the material associated with this mesh.
	pub fn material(&self) -> Result<String> {
		let mesh = &self.file.meshes[self.mesh_index];
		let name_offset = self.file.material_name_offsets[usize::from(mesh.material_index)];

		// todo: this logic should probably be abstracted in the structs impl, and the buffer hidden?
		let mut cursor = Cursor::new(&self.file.string_buffer);
		cursor.set_position(name_offset.into());

		let name = NullString::read(&mut cursor)?.to_string();
		Ok(name)
	}

	// TODO: iterator?
	/// Indices of vertices within the mesh. Vertices are laid out in a triangle list topology.
	pub fn indices(&self) -> Result<Vec<u16>> {
		// Get the offset of the indices within the file. The `start_index` on `mesh`
		// is representative of an already-ready array of u16, ergo *2.
		let mesh = &self.file.meshes[self.mesh_index];
		let offset = self.file.index_offset[usize::from(self.level)] + mesh.start_index * 2;

		// Read in the indices.
		let mut cursor = Cursor::new(&self.file.data);
		cursor.set_position(u64::from(offset) - self.file.data_offset);

		let indices = <Vec<u16>>::read_le_args(
			&mut cursor,
			VecArgs {
				count: mesh.index_count as usize,
				inner: (),
			},
		)?;

		Ok(indices)
	}

	// TODO: fn to get a specific attr?
	// TODO: iterator?
	/// Get the vertex attributes for all vertices in the mesh.
	pub fn attributes(&self) -> Result<Vec<VertexAttribute>> {
		let mesh = &self.file.meshes[self.mesh_index];

		// Get the elements for this mesh's vertices.
		let elements = &self.file.vertex_declarations[self.mesh_index].0;

		// Vertices are stored across multipe streams of data - set up a cursor for each. A mesh
		// can name more streams than it carries offsets for, so the offsets are what bound this.
		let mut streams = mesh.vertex_buffer_offset.map(|buffer_offset| {
			let cursor = Cursor::new(&self.file.data);
			let offset = self.file.vertex_offset[usize::from(self.level)] + buffer_offset;
			(cursor, u64::from(offset) - self.file.data_offset)
		});

		// Read in the vertices
		// TODO: keep an eye on perf here - could thrash cache a bit if llvm doesn't magic it enough
		elements
			.iter()
			.map(|element| -> Result<_> {
				let stream = usize::from(element.stream);
				let Some((cursor, base_offset)) = streams.get_mut(stream) else {
					return Err(Error::Invalid(
						ErrorValue::Other("model vertex element".into()),
						format!("element names stream {stream}, beyond the 3 a mesh carries"),
					));
				};
				let base_offset = *base_offset;
				let stride = u64::from(mesh.vertex_buffer_stride[stream]);

				let offsets = (0..mesh.vertex_count).scan(
					base_offset + u64::from(element.offset),
					|offset, _index| {
						let current = *offset;
						*offset += stride;
						Some(current)
					},
				);

				use VertexValues as V;
				use structs::VertexFormat as K;
				let values = match &element.format {
					K::Single3 => V::Vector3(read_values(offsets, cursor, single3)?),
					K::Single4 => V::Vector4(read_values(offsets, cursor, single4)?),
					K::Uint => V::Uint(read_values(offsets, cursor, uint)?),
					K::ByteFloat4 => V::Vector4(read_values(offsets, cursor, bfloat4)?),
					K::Half2 => V::Vector2(read_values(offsets, cursor, half2)?),
					K::Half4 => V::Vector4(read_values(offsets, cursor, half4)?),
					K::UByte8 => V::Bytes8(read_values(offsets, cursor, ubyte8)?),
					K::None => {
						return Err(Error::Invalid(
							ErrorValue::Other("model vertex element".into()),
							"element declares no format".into(),
						));
					}
				};

				Ok(VertexAttribute {
					kind: element.attribute,
					format: element.format,
					usage_index: element.usage_index,
					values,
				})
			})
			.collect::<Result<Vec<_>>>()
	}
}

fn read_values<R, F, O>(
	offsets: impl Iterator<Item = u64>,
	reader: &mut R,
	map_fn: F,
) -> Result<Vec<O>>
where
	R: Read + Seek,
	F: Fn(&mut R) -> Result<O>,
{
	offsets
		.map(|offset| {
			reader.seek(SeekFrom::Start(offset))?;
			map_fn(reader)
		})
		.collect::<Result<Vec<_>>>()
}

fn single3(reader: &mut (impl Read + Seek)) -> Result<[f32; 3]> {
	Ok([
		f32::read_le(reader)?,
		f32::read_le(reader)?,
		f32::read_le(reader)?,
	])
}

fn single4(reader: &mut (impl Read + Seek)) -> Result<[f32; 4]> {
	Ok([
		f32::read_le(reader)?,
		f32::read_le(reader)?,
		f32::read_le(reader)?,
		f32::read_le(reader)?,
	])
}

fn uint(reader: &mut (impl Read + Seek)) -> Result<u32> {
	Ok(u32::read_le(reader)?)
}

fn bfloat4(reader: &mut (impl Read + Seek)) -> Result<[f32; 4]> {
	Ok([
		f32::from(u8::read(reader)?) / 255.,
		f32::from(u8::read(reader)?) / 255.,
		f32::from(u8::read(reader)?) / 255.,
		f32::from(u8::read(reader)?) / 255.,
	])
}

fn half2(reader: &mut (impl Read + Seek)) -> Result<[f32; 2]> {
	Ok([
		f16::from_bits(u16::read_le(reader)?).to_f32(),
		f16::from_bits(u16::read_le(reader)?).to_f32(),
	])
}

fn ubyte8(reader: &mut (impl Read + Seek)) -> Result<[u8; 8]> {
	Ok(<[u8; 8]>::read(reader)?)
}

fn half4(reader: &mut (impl Read + Seek)) -> Result<[f32; 4]> {
	Ok([
		f16::from_bits(u16::read_le(reader)?).to_f32(),
		f16::from_bits(u16::read_le(reader)?).to_f32(),
		f16::from_bits(u16::read_le(reader)?).to_f32(),
		f16::from_bits(u16::read_le(reader)?).to_f32(),
	])
}

/// One part of a mesh, drawn with the rest of it but hideable on its own.
#[derive(Clone, Copy, Debug)]
pub struct Submesh {
	/// Where the part's indices start within its mesh's own.
	pub start: usize,
	/// How many indices it covers.
	pub count: usize,
	/// Bits of the model's [`attribute_names`](super::Model::attribute_names), which is the only
	/// name a part carries.
	pub attributes: u32,
}

// todo: public contents? - i mean, it makes sense to an extent.
/// A vertex attribute of a mesh.
#[derive(Debug)]
pub struct VertexAttribute {
	// todo i'm really not convinced on the name here
	/// The kind of data represented by this attribute.
	pub kind: structs::VertexAttributeKind,
	/// How the values were stored, which decides whether they arrived signed.
	pub format: structs::VertexFormat,
	/// Distinguishes attributes sharing a kind, such as a mesh's second UV set.
	pub usage_index: u8,
	/// Attribute data values.
	pub values: VertexValues,
}

/// Values of a vertex attribute.
#[allow(missing_docs)]
#[derive(Debug)]
pub enum VertexValues {
	Uint(Vec<u32>),
	Bytes8(Vec<[u8; 8]>),
	Vector2(Vec<[f32; 2]>),
	Vector3(Vec<[f32; 3]>),
	Vector4(Vec<[f32; 4]>),
}
