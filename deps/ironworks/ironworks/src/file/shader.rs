//! Shared code between shpk and shcd.

use std::io::Cursor;

use binrw::{BinRead, binread, meta::ReadEndian};
use getset::CopyGetters;

use crate::error::{Error, ErrorValue, Result};

/// A walk over a shader file, carrying where the last read finished.
pub(super) struct Walk<'a> {
	tag: &'static str,
	pub bytes: &'a [u8],
	pub at: usize,
}

impl<'a> Walk<'a> {
	pub fn new(tag: &'static str, bytes: &'a [u8]) -> Self {
		Self { tag, bytes, at: 0 }
	}

	pub fn invalid(&self, reason: impl Into<String>) -> Error {
		Error::Invalid(ErrorValue::Other(self.tag.into()), reason.into())
	}

	/// Read one record at the walk's position, leaving the walk after the bytes it took.
	pub fn read<T>(&mut self) -> Result<T>
	where
		T: for<'b> BinRead<Args<'b> = ()> + ReadEndian,
	{
		self.read_args(())
	}

	pub fn read_args<T, A>(&mut self, args: A) -> Result<T>
	where
		T: for<'b> BinRead<Args<'b> = A> + ReadEndian,
	{
		let rest = self.bytes.get(self.at..).ok_or_else(|| {
			self.invalid(format!("offset {:#x} is past the end of the file", self.at))
		})?;
		let mut cursor = Cursor::new(rest);
		let record = T::read_args(&mut cursor, args)?;
		self.at += usize::try_from(cursor.position()).expect("record is small");
		Ok(record)
	}

	/// Check the file is as long as its header claims.
	pub fn declared_size(&self, size: u32) -> Result<()> {
		match self.bytes.len() < to_usize(size) {
			true => Err(self.invalid(format!(
				"file declares {size} bytes but carries {}",
				self.bytes.len()
			))),
			false => Ok(()),
		}
	}

	/// The offsets of the bytecode `what` and of the string block, which must fall in the file in
	/// that order.
	pub fn sections(&self, blob: u32, strings: u32, what: &str) -> Result<(usize, usize)> {
		let blob = to_usize(blob);
		let strings = to_usize(strings);
		match blob > strings || strings > self.bytes.len() {
			true => Err(self.invalid(format!(
				"{what} {blob:#x} and string block {strings:#x} do not fall in the file in that order"
			))),
			false => Ok((blob, strings)),
		}
	}

	/// The extent `count` records of `size` bytes take from the walk's position, which is where the
	/// next table starts.
	pub fn extent(&self, count: usize, size: usize, what: &str) -> Result<usize> {
		self.extent_at(self.at, count, size, what)
	}

	/// As [`Walk::extent`], for a table the caller has already walked past the head of.
	pub fn extent_at(&self, at: usize, count: usize, size: usize, what: &str) -> Result<usize> {
		count
			.checked_mul(size)
			.and_then(|len| at.checked_add(len))
			.filter(|end| *end <= self.bytes.len())
			.ok_or_else(|| {
				self.invalid(format!(
					"{what} at {at:#x} declares {count} records, which do not fit in the file"
				))
			})
	}

	/// Read `count` fixed-size records, slicing to the table's own extent first so a record cannot
	/// read past the last one.
	pub fn table<T>(&mut self, count: usize, size: usize, what: &str) -> Result<Vec<T>>
	where
		T: for<'b> BinRead<Args<'b> = ()> + ReadEndian,
	{
		let end = self.extent(count, size, what)?;
		let mut cursor = Cursor::new(&self.bytes[self.at..end]);
		let records = (0..count)
			.map(|_| Ok(T::read(&mut cursor)?))
			.collect::<Result<Vec<_>>>()?;
		self.at = end;
		Ok(records)
	}

	pub fn resources(&mut self, count: usize, what: &str) -> Result<Vec<Resource>> {
		Ok(self
			.table::<ResourceRecord>(count, ResourceRecord::SIZE, what)?
			.into_iter()
			.map(Resource::from)
			.collect())
	}

	/// Check the walk landed exactly where `what` begins.
	pub fn ends_at(&self, at: usize, what: &str) -> Result<()> {
		match self.at == at {
			true => Ok(()),
			false => Err(self.invalid(format!(
				"tables end at {:#x}, where the {what} starts at {at:#x}",
				self.at
			))),
		}
	}
}

/// The resource counts on a shader entry.
#[binread]
#[br(little, import(extended: bool))]
#[derive(Debug)]
pub(super) struct Counts {
	pub constants: u16,
	pub samplers: u16,

	#[br(if(extended))]
	pub uavs: u16,
	#[br(if(extended))]
	pub textures: u16,
}

/// Where a shader's flat resource list divides, as running totals in the order the records are laid
/// out: constants, samplers, textures, unordered access views.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bands([usize; 4]);

impl Bands {
	pub fn total(&self) -> usize {
		self.0[3]
	}

	pub fn constants<'a>(&self, resources: &'a [Resource]) -> &'a [Resource] {
		&resources[..self.0[0]]
	}

	pub fn samplers<'a>(&self, resources: &'a [Resource]) -> &'a [Resource] {
		&resources[self.0[0]..self.0[1]]
	}

	pub fn textures<'a>(&self, resources: &'a [Resource]) -> &'a [Resource] {
		&resources[self.0[1]..self.0[2]]
	}

	pub fn uavs<'a>(&self, resources: &'a [Resource]) -> &'a [Resource] {
		&resources[self.0[2]..]
	}
}

impl From<&Counts> for Bands {
	fn from(counts: &Counts) -> Self {
		let constants = usize::from(counts.constants);
		let samplers = constants + usize::from(counts.samplers);
		let textures = samplers + usize::from(counts.textures);
		Self([
			constants,
			samplers,
			textures,
			textures + usize::from(counts.uavs),
		])
	}
}

#[binread]
#[br(little)]
#[derive(Debug)]
struct ResourceRecord {
	id: u32,
	string_offset: u32,
	string_length: u16,
	kind: u16,
	slot: u16,
	size: u16,
}

impl ResourceRecord {
	const SIZE: usize = 16;
}

/// A constant buffer, sampler, texture or unordered access view.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Resource {
	/// The crc32 of the resource's name.
	id: u32,

	string_offset: u32,
	string_length: u16,

	/// 0 = constant buffer, 1 = texture, 2 = everything else
	kind: u16,

	/// The register the resource binds to.
	slot: u16,

	/// Registers of 16 bytes for a constant buffer, and one for a resource of kind 2. On a
	/// sampler or texture it is not a size at all but an index, numbering the entry within its own
	/// band, or `0xFFFF` for none. A .shpk leaves it 0 in the tables it shares between shaders.
	size: u16,
}

impl From<ResourceRecord> for Resource {
	fn from(record: ResourceRecord) -> Self {
		Self {
			id: record.id,
			string_offset: record.string_offset,
			string_length: record.string_length,
			kind: record.kind,
			slot: record.slot,
			size: record.size,
		}
	}
}

/// Which DirectX a shader was compiled for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectX {
	Dx9,
	Dx11,
	Unknown([u8; 4]),
}

impl From<[u8; 4]> for DirectX {
	fn from(value: [u8; 4]) -> Self {
		match &value {
			b"DX9\0" => Self::Dx9,
			b"DX11" => Self::Dx11,
			_ => Self::Unknown(value),
		}
	}
}

/// The name a resource points at, or `None` where it points outside the string block.
pub(super) fn name<'a>(strings: &'a [u8], resource: &Resource) -> Option<&'a str> {
	let start = to_usize(resource.string_offset);
	let end = start.checked_add(usize::from(resource.string_length))?;
	std::str::from_utf8(strings.get(start..end)?).ok()
}

pub(super) fn to_usize(value: u32) -> usize {
	usize::try_from(value).expect("u32 fits usize")
}
