use std::fmt;

use crate::error::Result;

use super::{curve::CurveKey, invalid};

/// How deep blocks may nest before the file is rejected.
const DEPTH: usize = 16;

/// The tags whose payload is a record of its own rather than a value or nested blocks. Text and
/// clips can both be mistaken for well-formed blocks.
const OPAQUE: [&str; 4] = ["Tex", "SdNm", "Name", "Clip"];

/// The tag naming a block, up to four characters.
///
/// The file writes tags back to front and pads them with nulls, so `Ver` is stored as `reV\0`.
/// This is the tag the right way round.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name {
	bytes: [u8; 4],
	length: u8,
}

impl Name {
	fn parse(raw: [u8; 4]) -> Result<Self> {
		let length = raw.iter().position(|&byte| byte == 0).unwrap_or(4);
		let head = &raw[..length];
		if head.is_empty()
			|| !head.iter().all(|byte| (0x21..0x7f).contains(byte))
			|| raw[length..].iter().any(|&byte| byte != 0)
		{
			return Err(invalid(format!("unusable block tag {raw:02x?}")));
		}

		let mut bytes = [0; 4];
		for (index, &byte) in head.iter().rev().enumerate() {
			bytes[index] = byte;
		}
		Ok(Self {
			bytes,
			length: u8::try_from(length).unwrap(),
		})
	}

	/// The tag as text.
	pub fn as_str(&self) -> &str {
		std::str::from_utf8(&self.bytes[..usize::from(self.length)]).unwrap()
	}
}

impl fmt::Display for Name {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(self.as_str())
	}
}

impl fmt::Debug for Name {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "{:?}", self.as_str())
	}
}

impl PartialEq<str> for Name {
	fn eq(&self, other: &str) -> bool {
		self.as_str() == other
	}
}

impl PartialEq<&str> for Name {
	fn eq(&self, other: &&str) -> bool {
		self.as_str() == *other
	}
}

/// What a block carries.
#[derive(Debug)]
pub enum Payload {
	/// Nested blocks.
	Blocks(Vec<Block>),

	/// The keyframes of one curve.
	Keys(Vec<CurveKey>),

	/// Anything else, as written. Scalars and text land here; read them with
	/// [`i32`](Block::i32), [`f32`](Block::f32), [`bool`](Block::bool) or [`text`](Block::text).
	Bytes(Vec<u8>),
}

/// One tagged, length-prefixed record.
///
/// Whether a block nests is a property of its tag, which is open-ended, so ironworks nests where
/// the payload divides exactly into well-formed blocks and keeps the bytes otherwise.
#[derive(Debug)]
pub struct Block {
	name: Name,
	payload: Payload,
}

impl Block {
	/// The tag naming this block.
	pub fn name(&self) -> Name {
		self.name
	}

	/// What the block carries.
	pub fn payload(&self) -> &Payload {
		&self.payload
	}

	/// The nested blocks, in file order, empty where the block carries something else.
	pub fn blocks(&self) -> &[Block] {
		match &self.payload {
			Payload::Blocks(blocks) => blocks,
			_ => &[],
		}
	}

	/// The first nested block carrying `name`.
	pub fn find(&self, name: &str) -> Option<&Block> {
		find(self.blocks(), name)
	}

	/// The payload as written, empty where the block carries something else.
	pub fn bytes(&self) -> &[u8] {
		match &self.payload {
			Payload::Bytes(bytes) => bytes,
			_ => &[],
		}
	}

	/// The payload as a signed integer, where it is four bytes wide.
	pub fn i32(&self) -> Option<i32> {
		self.scalar().map(i32::from_le_bytes)
	}

	/// The payload as a float, where it is four bytes wide.
	pub fn f32(&self) -> Option<f32> {
		self.scalar().map(f32::from_le_bytes)
	}

	/// The payload as a flag. Flags are written one or four bytes wide, depending on the tag.
	pub fn bool(&self) -> Option<bool> {
		match &self.payload {
			Payload::Bytes(bytes) if matches!(bytes.len(), 1 | 4) => {
				Some(bytes.iter().any(|&byte| byte != 0))
			}
			_ => None,
		}
	}

	/// The keyframes, where this block is a curve's key list.
	pub fn keys(&self) -> Option<&[CurveKey]> {
		match &self.payload {
			Payload::Keys(keys) => Some(keys),
			_ => None,
		}
	}

	/// The payload as text, up to the terminator the format writes it with.
	pub fn text(&self) -> Option<String> {
		match &self.payload {
			Payload::Bytes(bytes) => Some(text(bytes)),
			_ => None,
		}
	}

	pub(super) fn into_blocks(self) -> Vec<Block> {
		match self.payload {
			Payload::Blocks(blocks) => blocks,
			_ => Vec::new(),
		}
	}

	fn scalar(&self) -> Option<[u8; 4]> {
		match &self.payload {
			Payload::Bytes(bytes) => bytes.as_slice().try_into().ok(),
			_ => None,
		}
	}

	pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Self>> {
		Self::parse_at(bytes, 0)
	}

	fn parse_at(bytes: &[u8], depth: usize) -> Result<Vec<Self>> {
		if depth > DEPTH {
			return Err(invalid(format!("blocks nest more than {DEPTH} deep")));
		}

		let mut blocks = Vec::new();
		let mut at = 0;
		while at < bytes.len() {
			let (payload, next) = span(bytes, at)?;
			let name = Name::parse(bytes[at..at + 4].try_into().unwrap())?;
			let payload = &bytes[payload];

			let payload = if name == "Keys" {
				Payload::Keys(CurveKey::parse(payload)?)
			} else if !OPAQUE.contains(&name.as_str()) && tiles(payload) {
				Payload::Blocks(Self::parse_at(payload, depth + 1)?)
			} else {
				Payload::Bytes(payload.to_vec())
			};

			blocks.push(Self { name, payload });
			at = next;
		}
		Ok(blocks)
	}
}

/// The first block in `blocks` carrying `name`.
pub(super) fn find<'a>(blocks: &'a [Block], name: &str) -> Option<&'a Block> {
	blocks.iter().find(|block| block.name == *name)
}

/// The payload of the block at `at`, and where the block after it begins.
fn span(bytes: &[u8], at: usize) -> Result<(std::ops::Range<usize>, usize)> {
	let header = bytes
		.get(at..at + 8)
		.ok_or_else(|| invalid(format!("block at {at:#x} runs past the end of its parent")))?;
	let size = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;

	// A size is read before anything vouches for it, and `tiles` reads one out of arbitrary bytes
	// on purpose, so sizes that overrun a 32-bit usize arrive in the ordinary course of parsing.
	let start = at + 8;
	let next = size
		.checked_next_multiple_of(4)
		.and_then(|size| start.checked_add(size))
		.filter(|&next| next <= bytes.len())
		.ok_or_else(|| {
			invalid(format!(
				"block at {at:#x} declares {size} bytes, more than its parent holds"
			))
		})?;
	Ok((start..start + size, next))
}

/// Whether a payload divides exactly into well-formed blocks.
fn tiles(bytes: &[u8]) -> bool {
	let mut at = 0;
	while at < bytes.len() {
		let Ok((_, next)) = span(bytes, at) else {
			return false;
		};
		if Name::parse(bytes[at..at + 4].try_into().unwrap()).is_err() {
			return false;
		}
		at = next;
	}
	at > 0
}

/// A payload holding a string, which is written with a terminator.
fn text(bytes: &[u8]) -> String {
	let end = bytes
		.iter()
		.position(|&byte| byte == 0)
		.unwrap_or(bytes.len());
	String::from_utf8_lossy(&bytes[..end]).into_owned()
}
