use std::fmt::Debug;

use binrw::{binread, helpers::until_eof};

/// Sparse table of `u64` entries keyed by set ID, shared by .eqp and .gmp.
#[binread]
#[br(little)]
pub struct BlockTable {
	#[br(restore_position)]
	control: u64,

	#[br(parse_with = until_eof)]
	data: Vec<u8>,
}

const BLOCK_SIZE: u16 = 160;
const BLOCK_COUNT: u16 = 64;

impl BlockTable {
	/// Get the entry for the specified set ID, falling back to `default` for a set whose block is
	/// omitted from the file.
	pub fn entry(&self, id: u16, default: u64) -> u64 {
		// Set 0 does not exist - entry 0 is the control word - and the game reads set 1 in its place.
		let id = id.max(1);

		let block = id / BLOCK_SIZE;
		if block >= BLOCK_COUNT {
			return default;
		}

		let bit = 1u64 << block;
		if self.control & bit == 0 {
			return default;
		}

		// Omitted blocks take no space, so a block's position is the count of those below it.
		let index = usize::from(BLOCK_SIZE) * (self.control & (bit - 1)).count_ones() as usize
			+ usize::from(id % BLOCK_SIZE);

		let at = index * 8;
		self.data.get(at..at + 8).map_or(default, |entry| {
			u64::from_le_bytes(entry.try_into().unwrap())
		})
	}
}

impl Debug for BlockTable {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BlockTable")
			.field("control", &format_args!("{:#018x}", self.control))
			.field("blocks", &self.control.count_ones())
			.finish()
	}
}
