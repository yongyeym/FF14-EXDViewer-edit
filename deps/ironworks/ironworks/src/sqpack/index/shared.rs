use std::{collections::BTreeSet, fmt};

use binrw::BinRead;

#[derive(BinRead, Debug)]
#[br(little, magic = b"SqPack\0\0")]
pub struct SqPackHeader {
	_platform_id: u8,
	// unknown: [u8; 3],
	#[br(pad_before = 3)]
	pub size: u32,
	_version: u32,
	_kind: u32,
}

#[derive(BinRead, Debug)]
#[br(little)]
pub struct IndexHeader {
	_size: u32,
	_version: u32,
	pub index_data: Section,
	_data_file_count: u32,
	pub synonym_data: Section,
	_empty_block_data: Section,
	_dir_index_data: Section,
	_index_type: u32,

	#[br(pad_before = 656)] // reserved
	_digest: Digest,
}

#[derive(BinRead, Debug)]
#[br(little)]
pub struct Section {
	pub offset: u32,
	pub size: u32,
	_digest: Digest,
}

#[derive(BinRead)]
struct Digest([u8; 64]);

impl fmt::Debug for Digest {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let digest_string = self.0.map(|byte| format!("{:02x}", byte)).join(" ");
		formatter.write_str(&digest_string)
	}
}

#[derive(BinRead, Clone, Debug)]
#[br(map = Self::read)]
pub struct FileMetadata {
	/// The hash is shared by several files, listed individually in [`Synonym`] records.
	pub is_synonym: bool,
	pub data_file_id: u8,
	pub offset: u64,
}

impl FileMetadata {
	fn read(input: u32) -> Self {
		Self {
			is_synonym: (input & 0b1) == 0b1,
			data_file_id: ((input & 0b1110) >> 1) as u8,
			offset: (input as u64 & !0xF) * 0x08,
		}
	}
}

/// Synonyms resolve hash collisions.
#[derive(BinRead, Debug)]
#[br(little)]
pub struct Synonym {
	#[br(pad_before = 8)]
	pub file_metadata: FileMetadata,

	conflict_index: u32,

	#[br(count = 240, map = Self::read_path)]
	path: String,
}

impl Synonym {
	pub const SIZE: u32 = 256;

	fn read_path(bytes: Vec<u8>) -> String {
		let end = bytes
			.iter()
			.position(|byte| *byte == 0)
			.unwrap_or(bytes.len());
		String::from_utf8_lossy(&bytes[..end]).into_owned()
	}

	fn is_terminator(&self) -> bool {
		self.conflict_index == u32::MAX
	}

	pub fn live(records: Vec<Self>) -> Vec<Self> {
		records
			.into_iter()
			.filter(|record| !record.is_terminator())
			.collect()
	}
}

/// Resolve a located entry to the file it names, and estimate that file's size.
pub fn resolve(
	metadata: &FileMetadata,
	synonyms: &[Synonym],
	offsets: &BTreeSet<(u8, u64)>,
	path: Option<&str>,
) -> Option<(FileMetadata, Option<u64>)> {
	let metadata = match metadata.is_synonym {
		true => {
			let path = path?;
			&synonyms
				.iter()
				.find(|synonym| synonym.path == path)?
				.file_metadata
		}
		false => metadata,
	};

	// Look up the offset after this meta, if any exists. The result's data
	// file ID is double checked to ensure we don't return cross-dat offsets,
	// which can occur if the requested file is the last file in a dat, but
	// further dats exist.
	let size = offsets
		.range((metadata.data_file_id, metadata.offset + 1)..)
		.next()
		.and_then(|(dat_id, offset)| match *dat_id == metadata.data_file_id {
			true => Some(offset - metadata.offset),
			false => None,
		});

	Some((metadata.clone(), size))
}

#[cfg(test)]
pub mod test {
	use super::Synonym;

	const SQPACK_HEADER: u32 = 0x400;
	const INDEX_HEADER: u32 = 0x400;

	fn section(offset: u32, size: u32) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend(offset.to_le_bytes());
		out.extend(size.to_le_bytes());
		out.resize(72, 0);
		out
	}

	/// An index file holding the given entry and synonym tables, laid out as the live files are:
	/// each header padded out to its own declared size, then the two sections back to back.
	pub fn index_file(indexes: &[u8], synonyms: &[u8]) -> Vec<u8> {
		let index_offset = SQPACK_HEADER + INDEX_HEADER;
		let synonym_offset = index_offset + u32::try_from(indexes.len()).unwrap();

		let mut out = b"SqPack\0\0".to_vec();
		out.resize(12, 0);
		out.extend(SQPACK_HEADER.to_le_bytes());
		out.resize(usize::try_from(SQPACK_HEADER).unwrap(), 0);

		out.extend(INDEX_HEADER.to_le_bytes());
		out.extend(1u32.to_le_bytes());
		out.extend(section(index_offset, u32::try_from(indexes.len()).unwrap()));
		out.extend(1u32.to_le_bytes());
		out.extend(section(
			synonym_offset,
			u32::try_from(synonyms.len()).unwrap(),
		));
		out.resize(usize::try_from(index_offset).unwrap(), 0);

		out.extend(indexes);
		out.extend(synonyms);
		out
	}

	/// `trailing` lands after the path's terminator, where live records keep the tail of whatever
	/// longer path the slot held before.
	pub fn synonym(
		hash: u64,
		metadata: u32,
		conflict_index: u32,
		path: &str,
		trailing: &[u8],
	) -> Vec<u8> {
		let mut out = hash.to_le_bytes().to_vec();
		out.extend(metadata.to_le_bytes());
		out.extend(conflict_index.to_le_bytes());
		out.extend(path.as_bytes());
		out.push(0);
		out.extend(trailing);
		out.resize(usize::try_from(Synonym::SIZE).unwrap(), 0);
		out
	}

	pub fn terminator() -> Vec<u8> {
		synonym(0, 0, u32::MAX, "", &[])
	}
}
