use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result, file::File};

use super::structs::BoundingBox;

/// The meshes a zone streams its collision from, always written as `list.pcb`.
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
pub struct MeshList {
	#[br(temp)]
	count: u32,

	/// The volume every entry sits within.
	#[br(pad_after = 4)]
	#[get_copy = "pub"]
	bounds: BoundingBox,

	#[br(count = count)]
	entries: Vec<MeshListEntry>,
}

impl MeshList {
	/// A list carrying no entries, which is a length no mesh can be written in.
	pub(super) const EMPTY_SIZE: usize = 32;

	/// The meshes the zone is built from.
	pub fn entries(&self) -> &[MeshListEntry] {
		&self.entries
	}

	/// Name of an entry's mesh, in the directory the list itself sits in.
	pub fn mesh_file(id: u32) -> String {
		format!("tr{id:04}.pcb")
	}
}

impl File for MeshList {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// One mesh of a zone, named by id and bounded where it sits.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct MeshListEntry {
	/// Names the mesh file, which is [`MeshList::mesh_file`].
	id: u32,

	/// The volume the mesh covers, which is its root node's bounds.
	#[br(pad_after = 4)]
	bounds: BoundingBox,
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use crate::{error::Error, file::File};

	use super::MeshList;

	fn list(bounds: ([f32; 3], [f32; 3]), entries: &[(u32, f32)]) -> Vec<u8> {
		let mut bytes = u32::try_from(entries.len()).unwrap().to_le_bytes().to_vec();
		for axis in bounds.0.into_iter().chain(bounds.1) {
			bytes.extend(axis.to_le_bytes());
		}
		bytes.extend([0; 4]);
		for &(id, at) in entries {
			bytes.extend(id.to_le_bytes());
			for axis in [at, at, at, at + 1.0, at + 1.0, at + 1.0] {
				bytes.extend(axis.to_le_bytes());
			}
			bytes.extend([0; 4]);
		}
		bytes
	}

	/// An entry's padding follows its bounds rather than preceding them, which Physis has the other
	/// way around and so reads every bound one field early.
	#[test]
	fn reads_bounds_after_the_id() {
		let file = MeshList::read(Cursor::new(list(
			([-1.0; 3], [8.0; 3]),
			&[(413, 2.0), (9, 5.0)],
		)))
		.unwrap();

		assert_eq!(file.bounds().min(), [-1.0; 3]);
		assert_eq!(file.bounds().max(), [8.0; 3]);
		assert_eq!(file.entries().len(), 2);
		assert_eq!(file.entries()[0].id(), 413);
		assert_eq!(file.entries()[0].bounds().min(), [2.0; 3]);
		assert_eq!(file.entries()[0].bounds().max(), [3.0; 3]);
		assert_eq!(file.entries()[1].bounds().min(), [5.0; 3]);
		assert_eq!(MeshList::mesh_file(file.entries()[0].id()), "tr0413.pcb");
	}

	#[test]
	fn rejects_a_truncated_entry() {
		let mut bytes = list(([0.0; 3], [1.0; 3]), &[(1, 0.0)]);
		bytes.truncate(bytes.len() - 8);
		assert!(matches!(
			MeshList::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
