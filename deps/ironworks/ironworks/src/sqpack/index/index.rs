use std::sync::{Arc, Mutex};

use binrw::BinRead;
use getset::CopyGetters;

use crate::{
	error::{Error, ErrorValue, Result},
	sqpack::Resource,
};

use super::{index1::Index1, index2::Index2, shared::FileMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexHash {
	/// `.index`
	Split(u64),
	/// `.index2`
	Whole(u32),
}

impl IndexHash {
	pub fn of(path: &str) -> (Option<Self>, Self) {
		(
			Index1::hash(path).map(Self::Split),
			Self::Whole(Index2::hash(path)),
		)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexEntry {
	pub repository: u8,
	pub category: u8,
	pub chunk: u8,
	pub hash: IndexHash,
}

/// Specifier of a file location within a SqPack category.
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Location {
	/// SqPack chunk the file is in, i.e. `0000XX.win32.dat1`.
	chunk: u8,
	/// Data file the file is in, i.e. `000000.win32.datX`.
	data_file: u8,
	/// Offset within the targeted data file that the file starts at.
	offset: u64,
	/// Estimated size of the target file, if known. This will typically err on
	/// the larger side, as files commonly have some amount of padding at the end.
	size: Option<u64>,
}

#[derive(Debug)]
pub struct Index<R> {
	repository: u8,
	category: u8,

	resource: Arc<R>,
	max_chunk: Mutex<Option<u16>>,
	chunks: Mutex<Vec<Arc<IndexChunk>>>,
}

impl<R: Resource> Index<R> {
	pub fn new(repository: u8, category: u8, resource: Arc<R>) -> Result<Self> {
		Ok(Self {
			repository,
			category,
			resource,
			max_chunk: None.into(),
			chunks: Vec::new().into(),
		})
	}

	/// Every file this category records, across all of its chunks.
	pub fn entries(&self) -> Result<Vec<IndexEntry>> {
		let mut entries = Vec::new();
		for chunk in self.chunks() {
			let (index, chunk) = chunk?;
			let repository = self.repository;
			let category = self.category;
			match &*chunk {
				IndexChunk::Index1(file) => entries.extend(file.hashes().map(|hash| IndexEntry {
					repository,
					category,
					chunk: index,
					hash: IndexHash::Split(hash),
				})),
				IndexChunk::Index2(file) => entries.extend(file.hashes().map(|hash| IndexEntry {
					repository,
					category,
					chunk: index,
					hash: IndexHash::Whole(hash),
				})),
			}
		}
		Ok(entries)
	}

	pub fn find(&self, path: &str) -> Result<Location> {
		let location = self.chunks().find_map(|chunk| {
			let (index, chunk) = match chunk {
				Ok(value) => value,
				Err(error) => return Some(Err(error)),
			};

			match chunk.find(path) {
				Err(Error::NotFound(_)) => None,
				Err(error) => Some(Err(error)),
				Ok((meta, size)) => Some(Ok(Location {
					chunk: index,
					data_file: meta.data_file_id,
					offset: meta.offset,
					size,
				})),
			}
		});

		match location {
			None => Err(Error::NotFound(ErrorValue::Path(path.into()))),
			Some(result) => result,
		}
	}

	fn chunks(&self) -> impl Iterator<Item = Result<(u8, Arc<IndexChunk>)>> + '_ {
		// Get the max known chunk ID. If we don't know it, we want to loop the full potential ID space (u8).
		let guard = self.max_chunk.lock().unwrap();
		let max_chunk = guard.unwrap_or(256);
		drop(guard);

		(0u16..max_chunk).map_while(|index| {
			let index_usize = usize::from(index);
			let index_u8 = u8::try_from(index).unwrap();

			// If we've already loaded this chunk index, use that.
			let guard = self.chunks.lock().unwrap();
			if let Some(chunk) = guard.get(index_usize) {
				return Some(Ok((index_u8, chunk.clone())));
			}
			drop(guard);

			// Try to build a new chunk.
			let chunk = IndexChunk::new(
				self.repository,
				self.category,
				index.try_into().unwrap(),
				&*self.resource,
			);

			match chunk {
				// Found an index - save it out to the cache.
				Ok(chunk) => {
					let mut guard = self.chunks.lock().unwrap();
					// The lock was released while reading, so another lookup may have stored this
					// chunk already.
					if guard.len() == index_usize {
						guard.push(chunk.into());
					}
					guard
						.get(index_usize)
						.map(|chunk| Ok((index_u8, chunk.clone())))
				}

				// No index was found for this chunk - mark index as the max chunk point so we don't do that again.
				Err(Error::NotFound(_)) => {
					*self.max_chunk.lock().unwrap() = Some(index);
					None
				}

				// Some other error occured, surface it.
				Err(error) => Some(Err(error)),
			}
		})
	}
}

#[derive(Debug)]
enum IndexChunk {
	Index1(Index1),
	Index2(Index2),
}

impl IndexChunk {
	fn new<R: Resource>(repository: u8, category: u8, chunk: u8, resource: &R) -> Result<Self> {
		let index1 = resource
			.index(repository, category, chunk)
			.and_then(|mut reader| Ok(IndexChunk::Index1(Index1::read(&mut reader)?)));

		match index1 {
			Err(Error::NotFound(_)) => resource
				.index2(repository, category, chunk)
				.and_then(|mut reader| Ok(IndexChunk::Index2(Index2::read(&mut reader)?))),
			result => result,
		}
	}

	fn find(&self, path: &str) -> Result<(FileMetadata, Option<u64>)> {
		match self {
			Self::Index1(index) => index.find(path),
			Self::Index2(index) => index.find(path),
		}
	}
}
