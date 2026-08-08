use std::{collections::BTreeSet, io::SeekFrom};

use binrw::binread;

use crate::error::{Error, ErrorValue, Result};

use super::{
	crc::crc32,
	shared::{self, FileMetadata, IndexHeader, SqPackHeader, Synonym},
};

#[binread]
#[derive(Debug)]
#[br(little)]
struct Entry {
	hash: u32,
	file_metadata: FileMetadata,
}

impl Entry {
	const SIZE: u32 = 8;
}

#[binread]
#[derive(Debug)]
#[br(little)]
pub struct Index2 {
	#[br(temp)]
	sqpack_header: SqPackHeader,

	#[br(temp, seek_before = SeekFrom::Start(sqpack_header.size.into()))]
	index_header: IndexHeader,

	#[br(
    seek_before = SeekFrom::Start(index_header.index_data.offset.into()),
    count = index_header.index_data.size / Entry::SIZE,
  )]
	indexes: Vec<Entry>,

	#[br(
    seek_before = SeekFrom::Start(index_header.synonym_data.offset.into()),
    count = index_header.synonym_data.size / Synonym::SIZE,
    map = Synonym::live,
  )]
	synonyms: Vec<Synonym>,

	#[br(calc = indexes.iter()
		.map(|entry| &entry.file_metadata)
		.chain(synonyms.iter().map(|synonym| &synonym.file_metadata))
		.map(|metadata| (metadata.data_file_id, metadata.offset))
		.collect())]
	offsets: BTreeSet<(u8, u64)>,
}

impl Index2 {
	/// The hash of every file recorded in this chunk, in the order the chunk stores them.
	pub fn hashes(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
		self.indexes.iter().map(|entry| entry.hash)
	}

	pub fn hash(path: &str) -> u32 {
		crc32(path.as_bytes())
	}

	pub fn find(&self, path: &str) -> Result<(FileMetadata, Option<u64>)> {
		self.locate(Self::hash(path), Some(path))
			.ok_or_else(|| Error::NotFound(ErrorValue::Path(path.into())))
	}

	pub fn find_hash(&self, hash: u32) -> Option<(FileMetadata, Option<u64>)> {
		self.locate(hash, None)
	}

	/// Whether the chunk records the hash as shared between colliding files, which is the one case
	/// a lookup by hash alone cannot answer even though the file is present.
	pub fn is_shared(&self, hash: u32) -> bool {
		self.indexes
			.binary_search_by_key(&hash, |entry| entry.hash)
			.is_ok_and(|found| self.indexes[found].file_metadata.is_synonym)
	}

	fn locate(&self, hash: u32, path: Option<&str>) -> Option<(FileMetadata, Option<u64>)> {
		let entry = self
			.indexes
			.binary_search_by_key(&hash, |entry| entry.hash)
			.map(|found| &self.indexes[found])
			.ok()?;

		shared::resolve(&entry.file_metadata, &self.synonyms, &self.offsets, path)
	}
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use binrw::BinRead;

	use crate::sqpack::index::shared::test::{index_file, synonym, terminator};

	use super::Index2;

	fn entry(hash: u32, metadata: u32) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(hash.to_le_bytes());
		out.extend(metadata.to_le_bytes());
		out
	}

	fn read(indexes: &[u8], synonyms: &[u8]) -> Index2 {
		Index2::read(&mut Cursor::new(index_file(indexes, synonyms))).unwrap()
	}

	#[test]
	fn finds_a_file_by_path_and_by_hash() {
		let root = Index2::hash("exd/root.exl");
		let item = Index2::hash("exd/item.exh");
		let mut entries = [(root, 0x10u32), (item, 0x20)];
		entries.sort_by_key(|(hash, _)| *hash);

		let indexes: Vec<u8> = entries
			.iter()
			.flat_map(|(hash, metadata)| entry(*hash, *metadata))
			.collect();
		let index = read(&indexes, &terminator());

		let (metadata, _) = index.find("exd/root.exl").unwrap();
		assert_eq!(metadata.offset, 0x10 * 8);

		let (metadata, _) = index.find_hash(item).unwrap();
		assert_eq!(metadata.offset, 0x20 * 8);
	}

	/// Synonym example, taken from real files: `060000.win32.index2` records both
	/// of these paths under `cb245d37`, and only the path recorded beside each one
	/// says which is which.
	#[test]
	fn colliding_paths_are_told_apart_by_the_recorded_path() {
		let icon = "ui/icon/150000/de/150751_hr1.tex";
		let uld = "ui/uld/turnbreaktitle.uld";
		let hash = Index2::hash(icon);
		assert_eq!(
			hash,
			Index2::hash(uld),
			"these paths collide in the live files"
		);

		let index = read(
			&entry(hash, 1),
			&[
				synonym(hash.into(), 0x40, 0, icon, b"leftover"),
				synonym(hash.into(), 0x80, 1, uld, &[]),
				terminator(),
			]
			.concat(),
		);

		let (metadata, _) = index.find(icon).unwrap();
		assert_eq!(metadata.offset, 0x40 * 8);

		let (metadata, _) = index.find(uld).unwrap();
		assert_eq!(metadata.offset, 0x80 * 8);

		// Neither can be picked out by the hash they share.
		assert!(index.find_hash(hash).is_none());
	}

	#[test]
	fn a_synonym_record_bounds_the_file_before_it() {
		let flagged = Index2::hash("exd/root.exl");
		let plain = Index2::hash("exd/item.exh");
		let mut entries = [(flagged, 1u32), (plain, 0x40)];
		entries.sort_by_key(|(hash, _)| *hash);

		let indexes: Vec<u8> = entries
			.iter()
			.flat_map(|(hash, metadata)| entry(*hash, *metadata))
			.collect();
		let index = read(
			&indexes,
			&[
				synonym(flagged.into(), 0x80, 0, "exd/root.exl", &[]),
				terminator(),
			]
			.concat(),
		);

		// The files a collision holds are absent from the main table, so leaving their records out
		// of the offsets would make the file in front of one look like it runs to the end of the
		// data file.
		let (_, size) = index.find("exd/item.exh").unwrap();
		assert_eq!(size, Some(0x40 * 8));
	}
}
