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
	hash: u64,
	#[br(pad_after = 4)]
	file_metadata: FileMetadata,
	// padding: u32,
}

impl Entry {
	const SIZE: u32 = 16;
}

#[binread]
#[derive(Debug)]
#[br(little)]
pub struct Index1 {
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

impl Index1 {
	/// The hash of every file recorded in this chunk, in the order the chunk stores them.
	pub fn hashes(&self) -> impl ExactSizeIterator<Item = u64> + '_ {
		self.indexes.iter().map(|entry| entry.hash)
	}

	pub fn hash(path: &str) -> Option<u64> {
		let mut segments = path
			.rsplitn(2, '/')
			.map(|segment| crc32(segment.as_bytes()));
		match (segments.next(), segments.next()) {
			(Some(file), Some(directory)) => Some(u64::from(directory) << 32 | u64::from(file)),
			_ => None,
		}
	}

	pub fn find(&self, path: &str) -> Result<(FileMetadata, Option<u64>)> {
		let hash = Self::hash(path).ok_or_else(|| {
			Error::Invalid(
				ErrorValue::Path(path.into()),
				"Paths must contain at least two segments.".into(),
			)
		})?;

		self.locate(hash, Some(path))
			.ok_or_else(|| Error::NotFound(ErrorValue::Path(path.into())))
	}

	pub fn find_hash(&self, hash: u64) -> Option<(FileMetadata, Option<u64>)> {
		self.locate(hash, None)
	}

	fn locate(&self, hash: u64, path: Option<&str>) -> Option<(FileMetadata, Option<u64>)> {
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

	use super::Index1;

	/// `metadata`: synonym bit, data file, and offset
	fn entry(hash: u64, metadata: u32) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(hash.to_le_bytes());
		out.extend(metadata.to_le_bytes());
		out.extend([0u8; 4]);
		out
	}

	fn read(indexes: &[u8], synonyms: &[u8]) -> Index1 {
		Index1::read(&mut Cursor::new(index_file(indexes, synonyms))).unwrap()
	}

	#[test]
	fn a_path_without_a_directory_has_no_hash() {
		assert_eq!(Index1::hash("root.exl"), None);
	}

	#[test]
	fn finds_a_file_by_path_and_by_hash() {
		let root = Index1::hash("exd/root.exl").unwrap();
		let item = Index1::hash("exd/item.exh").unwrap();
		let mut entries = [(root, 0x10u32), (item, 0x20)];
		entries.sort_by_key(|(hash, _)| *hash);

		let indexes: Vec<u8> = entries
			.iter()
			.flat_map(|(hash, metadata)| entry(*hash, *metadata))
			.collect();
		let index = read(&indexes, &terminator());

		let (metadata, size) = index.find("exd/root.exl").unwrap();
		assert_eq!((metadata.data_file_id, metadata.offset), (0, 0x10 * 8));
		assert_eq!(size, Some(0x10 * 8));

		let (metadata, _) = index.find_hash(item).unwrap();
		assert_eq!(metadata.offset, 0x20 * 8);

		assert!(index.find("exd/missing.exl").is_err());
	}

	#[test]
	fn a_flagged_entry_resolves_through_the_synonym_table() {
		let hash = Index1::hash("exd/root.exl").unwrap();
		let index = read(
			&entry(hash, 1),
			&[
				synonym(hash, 0x40, 0, "exd/root.exl", b"leftover"),
				synonym(hash, 0x80, 1, "exd/other.exl", &[]),
				terminator(),
			]
			.concat(),
		);

		let (metadata, _) = index.find("exd/root.exl").unwrap();
		assert_eq!(metadata.offset, 0x40 * 8);
		assert!(!metadata.is_synonym);
	}

	#[test]
	fn a_flagged_entry_is_not_resolved_by_hash_alone() {
		let hash = Index1::hash("exd/root.exl").unwrap();
		let index = read(
			&entry(hash, 1),
			&[synonym(hash, 0x40, 0, "exd/root.exl", &[]), terminator()].concat(),
		);

		// The entry carries data file 0 and offset 0, which is the start of the dat's own header.
		// Reporting nothing is the only honest answer.
		assert!(index.find_hash(hash).is_none());
	}

	#[test]
	fn the_terminator_does_not_name_a_file() {
		let hash = Index1::hash("exd/root.exl").unwrap();
		let index = read(&entry(hash, 1), &terminator());

		assert!(index.find("exd/root.exl").is_err());
	}
}
