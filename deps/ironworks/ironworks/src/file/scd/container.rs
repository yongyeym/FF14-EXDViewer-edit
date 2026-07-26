use std::io::{Cursor, Seek, SeekFrom};

use binrw::{BinRead, binread};
use derivative::Derivative;

use crate::{FileStream, error::Result, file::File};

use super::entry::SoundEntry;

/// A `.scd` sound container.
#[derive(Debug)]
pub struct SoundContainer {
	entries: Vec<SoundEntry>,
}

impl SoundContainer {
	/// The audio streams contained in this file.
	pub fn entries(&self) -> &[SoundEntry] {
		&self.entries
	}

	/// The audio stream at `index`, if present.
	pub fn sound(&self, index: usize) -> Option<&SoundEntry> {
		self.entries.get(index)
	}
}

impl File for SoundContainer {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		let mut cursor = Cursor::new(&bytes);

		let binary = BinaryHeader::read(&mut cursor)?;
		cursor.seek(SeekFrom::Start(binary.header_offset.into()))?;
		let header = ScdHeader::read(&mut cursor)?;

		cursor.seek(SeekFrom::Start(header.audio_offset.into()))?;
		let offsets = (0..header.audio_count)
			.map(|_| u32::read_le(&mut cursor))
			.collect::<binrw::BinResult<Vec<u32>>>()?;

		let entries = offsets
			.into_iter()
			.filter(|&offset| offset != 0)
			.map(|offset| SoundEntry::parse(&bytes, offset as usize))
			.collect::<Result<Vec<_>>>()?;

		Ok(Self { entries })
	}
}

#[binread]
#[br(little, magic = b"SEDBSSCF")]
#[derive(Derivative)]
#[derivative(Debug)]
struct BinaryHeader {
	version: u32,
	endian: u8,
	align: u8,
	header_offset: u16,
	file_size: u64,
}

#[binread]
#[br(little)]
#[derive(Derivative)]
#[derivative(Debug)]
struct ScdHeader {
	sound_count: u16,
	track_count: u16,
	audio_count: u16,
	number: u16,
	track_offset: u32,
	audio_offset: u32,
	layout_offset: u32,
	routing_offset: u32,
	attribute_offset: u32,
}
