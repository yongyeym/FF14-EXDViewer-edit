use binrw::binread;

#[binread]
#[derive(Debug)]
#[br(little)]
pub struct Header {
	pub size: u32,
	pub kind: FileKind,
	pub raw_file_size: u32,
	// num_blocks: u32,
	// block_buffer_size: u32,
	#[br(pad_before = 8)]
	pub block_count: u32,
}

/// How sqpack stores a file's data.
#[binread]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[br(little, repr = u32)]
pub enum FileKind {
	/// No data at all: the entry is a header and nothing else.
	Empty = 1,
	Standard,
	Model,
	Texture,
}

impl FileKind {
	/// The name sqpack knows this kind by.
	pub fn name(self) -> &'static str {
		match self {
			Self::Empty => "Empty",
			Self::Standard => "Standard",
			Self::Model => "Model",
			Self::Texture => "Texture",
		}
	}
}
