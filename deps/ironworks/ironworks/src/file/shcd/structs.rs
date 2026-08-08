use binrw::binread;

use crate::file::shader::Counts;

/// Versions from this one on carry textures and unordered access views, and a word per shader entry.
pub const VERSION_RESOURCES: u32 = 0x0601;

/// The file's root header. The version is three bytes wide, the fourth carrying the stage, so a
/// version reads a byte narrower than the .shpk one it otherwise resembles.
#[binread]
#[br(little, magic = b"ShCd")]
#[derive(Debug)]
pub struct Header {
	pub version: [u8; 3],
	pub stage: u8,

	pub directx: [u8; 4],

	pub total_size: u32,

	pub blob_offset: u32,
	pub strings_offset: u32,
}

#[binread]
#[br(little, import(version: u32))]
#[derive(Debug)]
pub struct Shader {
	pub blob_offset: u32,
	pub blob_size: u32,

	#[br(args(version >= VERSION_RESOURCES))]
	pub counts: Counts,

	#[br(if(version >= VERSION_RESOURCES))]
	_unknown: u32,
}
