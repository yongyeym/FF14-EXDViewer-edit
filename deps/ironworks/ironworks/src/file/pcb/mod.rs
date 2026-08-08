//! Structs and utilities for parsing .pcb files.

mod list;
mod mesh;
mod structs;

pub use {
	list::{MeshList, MeshListEntry},
	mesh::{Mesh, Node},
	structs::{BoundingBox, MaterialWidth, Primitive},
};

use std::io::Cursor;

use binrw::BinRead;

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("PCB".into()), reason.into())
}

/// The two formats the `.pcb` extension is used for.
///
/// A mesh writes zero where a list writes its entry count, so the two are told apart by content
/// rather than by name. A list carrying no entries writes that same zero, and is told apart by
/// being 32 bytes long, which is too short to hold a mesh's header and root node.
#[derive(Debug)]
pub enum Collision {
	/// Collision geometry.
	Mesh(Mesh),

	/// The meshes a zone streams its collision from.
	List(MeshList),
}

impl File for Collision {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;

		let header = structs::Header::read(&mut Cursor::new(&bytes))?;
		if header.kind == 0 && bytes.len() != MeshList::EMPTY_SIZE {
			return Ok(Self::Mesh(
				Mesh::parse(&bytes, MaterialWidth::Wide)
					.or_else(|_| Mesh::parse(&bytes, MaterialWidth::Narrow))?,
			));
		}
		Ok(Self::List(<MeshList as BinRead>::read(&mut Cursor::new(
			&bytes,
		))?))
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Collision, MaterialWidth, structs};

	/// A header and one empty leaf, which is the shortest a mesh can be written in.
	fn mesh() -> Vec<u8> {
		let mut bytes = vec![0; structs::Header::SIZE + 0x30];
		bytes[4] = 1;
		bytes
	}

	fn mesh_with_width(width: MaterialWidth) -> Vec<u8> {
		let mut bytes = mesh();
		let mut node = Vec::new();
		node.extend([0u8; 8]);
		node.extend([0i32; 2].into_iter().flat_map(i32::to_le_bytes));
		node.extend([0.0f32; 6].into_iter().flat_map(f32::to_le_bytes));
		node.extend([0u16, 0, 3].into_iter().flat_map(u16::to_le_bytes));
		node.extend([0u8; 2]);
		node.extend([0.0f32; 9].into_iter().flat_map(f32::to_le_bytes));
		node.extend([0u8, 1, 2, 0]);
		match width {
			MaterialWidth::Narrow => node.extend(0x7004u16.to_le_bytes()),
			MaterialWidth::Wide => node.extend(0x7004u64.to_le_bytes()),
		}
		bytes.extend(node);
		bytes
	}

	fn list(entries: usize) -> Vec<u8> {
		let mut bytes = vec![0; 32 + 32 * entries];
		bytes[0] = u8::try_from(entries).unwrap();
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			Collision::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn a_mesh_and_a_list_are_told_apart_by_content() {
		assert!(matches!(
			Collision::read(Cursor::new(mesh())),
			Ok(Collision::Mesh(_))
		));
		assert!(matches!(
			Collision::read(Cursor::new(mesh_with_width(MaterialWidth::Narrow))),
			Ok(Collision::Mesh(_))
		));
		assert!(matches!(
			Collision::read(Cursor::new(list(1))),
			Ok(Collision::List(_))
		));
	}

	/// An empty list writes the same leading zero a mesh does, and only its length tells.
	#[test]
	fn an_empty_list_is_not_read_as_a_mesh() {
		let file = Collision::read(Cursor::new(list(0))).unwrap();
		let Collision::List(list) = file else {
			panic!("read as a mesh")
		};
		assert!(list.entries().is_empty());
	}
}
