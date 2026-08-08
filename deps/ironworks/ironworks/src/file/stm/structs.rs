use binrw::binread;

/// Header and lookup tables of a staining template file.
#[binread]
#[br(little, magic = 0x534Du16)]
#[derive(Debug)]
pub struct Header {
	pub version: u16,

	#[br(temp)]
	entry_count: u16,

	/// Columns of three halves, then columns of one, that each entry carries. Zero on v0x0101,
	/// which predates the counts being stated.
	pub color_count: u8,
	pub scalar_count: u8,

	#[br(count = entry_count)]
	pub keys: Vec<u32>,

	/// Where each entry starts, in units of two bytes from the end of this table.
	#[br(count = entry_count)]
	pub offsets: Vec<u32>,
}
