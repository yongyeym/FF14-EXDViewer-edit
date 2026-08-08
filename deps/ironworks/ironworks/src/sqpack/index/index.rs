use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use binrw::BinRead;
use getset::CopyGetters;

use crate::{
	error::{Error, ErrorValue, Result},
	sqpack::Resource,
};

use super::{index1::Index1, index2::Index2, shared::FileMetadata};

const CHUNK_MISS_TOLERANCE: u16 = 4;

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

	/// The hash of a directory on its own (upper half of [`Split`](Self::Split))
	pub fn directory(path: &str) -> u32 {
		super::crc::crc32(path.as_bytes())
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

impl Location {
	fn new(chunk: u8, (metadata, size): (FileMetadata, Option<u64>)) -> Self {
		Self {
			chunk,
			data_file: metadata.data_file_id,
			offset: metadata.offset,
			size,
		}
	}
}

/// The chunk a path is stored in.
fn path_chunk(repository: u8, path: &str) -> u8 {
	if repository == 0 {
		return 0;
	}

	path.split('/')
		.nth(2)
		.and_then(|zone| zone.get(..2))
		.filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
		.and_then(|digits| digits.parse().ok())
		.unwrap_or(0)
}

#[derive(Debug)]
pub struct Index<R> {
	repository: u8,
	category: u8,

	resource: Arc<R>,
	max_chunk: Mutex<Option<u16>>,
	chunks: Mutex<HashMap<u8, Option<Arc<IndexChunk>>>>,
	whole: Mutex<HashMap<u8, Option<Arc<Index2>>>>,
}

impl<R: Resource> Index<R> {
	pub fn new(repository: u8, category: u8, resource: Arc<R>) -> Result<Self> {
		Ok(Self {
			repository,
			category,
			resource,
			max_chunk: None.into(),
			chunks: Default::default(),
			whole: Default::default(),
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

	/// Locate a file by its index hash.
	pub fn find_hash(&self, hash: IndexHash) -> Result<Location> {
		let not_found = || Error::NotFound(ErrorValue::Other(format!("hash {hash:?}")));

		let whole = match hash {
			// A split hash names at most one file across a category's chunks, so the first chunk
			// to claim it is the answer and there is no reason to read the rest.
			IndexHash::Split(split) => {
				return self
					.chunks()
					.find_map(|chunk| {
						let (index, chunk) = match chunk {
							Ok(value) => value,
							Err(error) => return Some(Err(error)),
						};
						match &*chunk {
							IndexChunk::Index1(index1) => index1.find_hash(split),
							IndexChunk::Index2(_) => None,
						}
						.map(|located| Ok(Location::new(index, located)))
					})
					.unwrap_or_else(|| Err(not_found()));
			}
			IndexHash::Whole(whole) => whole,
		};

		let ambiguous = || {
			Error::Invalid(
				ErrorValue::Other(format!("hash {hash:?}")),
				"names more than one file, so it can only be read by path".into(),
			)
		};
		let look_up = |index2: &Index2| (index2.find_hash(whole), index2.is_shared(whole));

		let mut found = None;
		for chunk in self.chunks() {
			let (index, chunk) = chunk?;
			let (located, shared) = match &*chunk {
				IndexChunk::Index2(index2) => look_up(index2),
				IndexChunk::Index1(_) => self
					.whole_chunk(index)?
					.as_deref()
					.map_or((None, false), look_up),
			};

			if shared {
				return Err(ambiguous());
			}

			match (located, &found) {
				(Some(located), None) => found = Some(Location::new(index, located)),
				// The same whole-path hash can turn up in two chunks, where nothing at all records
				// which was meant.
				(Some(_), Some(_)) => return Err(ambiguous()),
				(None, _) => (),
			}
		}

		found.ok_or_else(not_found)
	}

	pub fn find(&self, path: &str) -> Result<Location> {
		let expected = path_chunk(self.repository, path);
		if let Some(chunk) = self.chunk(expected)? {
			if let Ok(located) = chunk.find(path) {
				return Ok(Location::new(expected, located));
			}
		}

		let location = self.chunks().find_map(|chunk| {
			let (index, chunk) = match chunk {
				Ok(value) => value,
				Err(error) => return Some(Err(error)),
			};

			if index == expected {
				return None;
			}

			match chunk.find(path) {
				Err(Error::NotFound(_)) => None,
				Err(error) => Some(Err(error)),
				Ok(located) => Some(Ok(Location::new(index, located))),
			}
		});

		match location {
			None => Err(Error::NotFound(ErrorValue::Path(path.into()))),
			Some(result) => result,
		}
	}

	/// The chunk with the given ID. `None` if the category has no such chunk.
	fn chunk(&self, chunk: u8) -> Result<Option<Arc<IndexChunk>>> {
		if let Some(known) = self.chunks.lock().unwrap().get(&chunk) {
			return Ok(known.clone());
		}

		let built = match IndexChunk::new(self.repository, self.category, chunk, &*self.resource) {
			Ok(built) => Some(Arc::new(built)),
			// Remembered as absent so a later lookup does not probe the resource for it again.
			Err(Error::NotFound(_)) => None,
			Err(error) => return Err(error),
		};

		self.chunks.lock().unwrap().insert(chunk, built.clone());
		Ok(built)
	}

	/// The `.index2` of a chunk that also ships an `.index`, read on first use.
	fn whole_chunk(&self, chunk: u8) -> Result<Option<Arc<Index2>>> {
		if let Some(known) = self.whole.lock().unwrap().get(&chunk) {
			return Ok(known.clone());
		}

		let built = match self.resource.index2(self.repository, self.category, chunk) {
			Ok(mut reader) => Some(Arc::new(Index2::read(&mut reader)?)),
			Err(Error::NotFound(_)) => None,
			Err(error) => return Err(error),
		};

		self.whole.lock().unwrap().insert(chunk, built.clone());
		Ok(built)
	}

	fn chunks(&self) -> impl Iterator<Item = Result<(u8, Arc<IndexChunk>)>> + '_ {
		// Get the max known chunk ID. If we don't know it, we want to loop the full potential ID space (u8).
		let max_chunk = self.max_chunk.lock().unwrap().unwrap_or(256);

		(0u16..max_chunk)
			.scan(0u16, |misses, index| {
				let id = u8::try_from(index).unwrap();

				match self.chunk(id) {
					Ok(Some(chunk)) => {
						*misses = 0;
						Some(Some(Ok((id, chunk))))
					}

					// Chunk IDs are not contiguous in live data, so a hole is not the end of the
					// category; only a run of them means we have walked off the end of it.
					Ok(None) => {
						*misses += 1;
						match *misses >= CHUNK_MISS_TOLERANCE {
							true => {
								*self.max_chunk.lock().unwrap() = Some(index + 1 - *misses);
								None
							}
							false => Some(None),
						}
					}

					// Some other error occured, surface it.
					Err(error) => Some(Some(Err(error))),
				}
			})
			.flatten()
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

#[cfg(test)]
mod tests {
	use super::{IndexHash, path_chunk};

	#[test]
	fn an_expansion_zone_names_its_chunk() {
		for (repository, path, chunk) in [
			(5, "bg/ex5/06_nvt_n6/cnt/n6c6/bgparts/n6c6_xx_bfi04.mdl", 6),
			(
				2,
				"bg/ex2/01_gyr_g3/fld/g3f2/collision/g3f2_t1_nat03.pcb",
				1,
			),
			(4, "bg/ex4/09_ocn_o5/common/vfx/eff/b2745dart1_f1.avfx", 9),
			// Everything an expansion ships outside a numbered zone shares chunk 0.
			(1, "cut/ex1/banall/banall00001.pap", 0),
			(4, "music/ex4/bgm_ex4_wks_01.scd", 0),
			// The base repository is chunk 0 throughout, digits in the path or not.
			(0, "bg/ffxiv/sea_s1/fld/s1f2/grass/038_004_012_l.ggd", 0),
			(0, "ui/icon/150000/de/150751_hr1.tex", 0),
			(0, "exd/root.exl", 0),
		] {
			assert_eq!(path_chunk(repository, path), chunk, "{path}");
		}
	}

	#[test]
	fn directory_matches_the_upper_half_of_a_split_hash() {
		for (dir, file) in [
			("music/ffxiv", "BGM_Null.scd"),
			("exd", "root.exl"),
			("common/savedata", "anything.dat"),
		] {
			let Some(IndexHash::Split(split)) = IndexHash::of(&format!("{dir}/{file}")).0 else {
				panic!("no split hash for {dir}/{file}");
			};
			assert_eq!(IndexHash::directory(dir), (split >> 32) as u32, "{dir}");
		}
	}
}
