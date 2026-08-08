//! Structs and utilities for parsing .tex files.

use binrw::helpers::until_eof;
use binrw::{BinRead, binread};
use derivative::Derivative;
use getset::{CopyGetters, Getters};
use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::{FileStream, error::Result};

use super::file::File;

/// A texture and associated metadata.
#[binread]
#[br(little)]
#[derive(Derivative, Getters, CopyGetters)]
#[derivative(Debug)]
pub struct Texture {
	attributes: u32,

	/// Pixel data format.
	#[get_copy = "pub"]
	format: Format,

	/// Width in pixels.
	#[get_copy = "pub"]
	width: u16,

	/// Height in pixels.
	#[get_copy = "pub"]
	height: u16,

	/// Depth. Unknown interpretation.
	#[get_copy = "pub"]
	depth: u16,

	/// Mipmap level count.
	#[get_copy = "pub"]
	#[br(map = |x: u8| x & 127)]
	mip_levels: u8,

	/// Texture array size. Only used by D2Array texture kinds.
	#[get_copy = "pub"]
	array_size: u8,

	// TODO: work out how these should be exposed.
	lod_surfaces: [u32; 3],
	surface_offsets: [u32; 13],

	/// Byte array of pixel data.
	#[br(parse_with = until_eof)]
	#[derivative(Debug = "ignore")]
	#[get = "pub"]
	data: Vec<u8>,
}

const HEADER_SIZE: u32 = 80;

impl Texture {
	/// Dimensions of a mipmap level. Each level halves, stopping at one pixel.
	pub fn mip_size(&self, level: u8) -> (u16, u16) {
		let shift = u32::from(level).min(u16::BITS - 1);
		((self.width >> shift).max(1), (self.height >> shift).max(1))
	}

	/// Pixel data for a single mipmap level, or `None` if the file does not have that level.
	pub fn mip_data(&self, level: u8) -> Option<&[u8]> {
		if level >= self.mip_levels {
			return None;
		}
		let offset = |level: u8| -> Option<usize> {
			let raw = *self.surface_offsets.get(usize::from(level))?;
			raw.checked_sub(HEADER_SIZE).map(|o| o as usize)
		};
		let start = offset(level)?;
		// The next level's offset bounds this one; the last runs to the end of the data.
		let end = match offset(level + 1) {
			Some(end) if level + 1 < self.mip_levels && end > start => end,
			_ => self.data.len(),
		};
		self.data.get(start..end.min(self.data.len()))
	}

	/// How many images the file stores at a mipmap level. A volume's depth halves along with its
	/// width and height, where a cube's six faces and an array's elements carry down the chain
	/// unchanged, so the level only matters for a volume.
	pub fn layers(&self, level: u8) -> u16 {
		match self.kind() {
			TextureKind::D3 => {
				let shift = u32::from(level).min(u16::BITS - 1);
				(self.depth >> shift).max(1)
			}
			TextureKind::Cube => 6,
			TextureKind::D2Array => u16::from(self.array_size).max(1),
			_ => 1,
		}
	}

	/// Kind of texture represented by this file.
	pub fn kind(&self) -> TextureKind {
		match (self.attributes & TextureKind::MASK) >> TextureKind::SHIFT {
			0b0000001 => TextureKind::D1,
			0b0000010 => TextureKind::D2,
			0b0000100 => TextureKind::D3,
			0b0001000 => TextureKind::Cube,
			0b1000000 => TextureKind::D2Array,
			_ => TextureKind::Unknown,
		}
	}
}

impl File for Texture {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// The kind of a texture, or resource. This value implies the semantics of the
/// rest of the texture metadata.
#[allow(missing_docs)]
#[derive(Debug, PartialEq, Eq)]
pub enum TextureKind {
	Unknown,
	D1,
	D2,
	D3,
	Cube,
	D2Array,
}

impl TextureKind {
	const SHIFT: u32 = 22;
	const MASK: u32 = 0x13C00000;
}

/// Pixel format of a texture.
#[allow(missing_docs)]
#[binread]
#[derive(Copy, Clone, Debug, PartialEq, Eq, IntoPrimitive)]
#[br(repr = u32)]
#[repr(u32)]
pub enum Format {
	// Names largely from, or derived from, Lumina. Comments reference the
	// DXGI_FORMAT used by the game where known. Some mismatches, don't ask me.
	// If it breaks, it's Winter's fault.
	Unknown = 0x0,

	// Integer
	L8Unorm = 0x1130,      // DXGI_FORMAT_B8G8R8A8_UNORM
	A8Unorm = 0x1131,      // DXGI_FORMAT_A8_UNORM
	R8Unorm = 0x1132,      // DXGI_FORMAT_R8_UNORM
	R8Uint = 0x1133,       // DXGI_FORMAT_R8_UINT
	R16Uint = 0x1140,      // DXGI_FORMAT_R16_UINT
	R32Uint = 0x1150,      // DXGI_FORMAT_R32_UINT
	Rg8Unorm = 0x1240,     // DXGI_FORMAT_R8G8_UNORM
	Bgra4Unorm = 0x1440,   // DXGI_FORMAT_B8G8R8A8_UNORM
	Bgr5a1Unorm = 0x1441,  // DXGI_FORMAT_B8G8R8A8_UNORM
	Bgra8Unorm = 0x1450,   // DXGI_FORMAT_B8G8R8A8_UNORM
	Bgrx8Unorm = 0x1451,   // DXGI_FORMAT_B8G8R8X8_UNORM
	Argb8Unknown = 0x1452, // Unknown

	// Float
	R16Float = 0x2140,    // DXGI_FORMAT_R16_FLOAT
	R32Float = 0x2150,    // DXGI_FORMAT_R32_FLOAT
	Rg16Float = 0x2250,   // DXGI_FORMAT_R16G16_FLOAT
	Rg32Float = 0x2260,   // DXGI_FORMAT_R32G32_FLOAT
	Rgba16Float = 0x2460, // DXGI_FORMAT_R16G16B16A16_FLOAT
	Rgba32Float = 0x2470, // DXGI_FORMAT_R32G32B32A32_FLOAT

	// Bcn1
	Bc1Unorm = 0x3420, // DXGI_FORMAT_BC1_UNORM
	Bc2Unorm = 0x3430, // DXGI_FORMAT_BC2_UNORM
	Bc3Unorm = 0x3431, // DXGI_FORMAT_BC3_UNORM

	// DepthStencil
	D16 = 0x4140,          // DXGI_FORMAT_R16_TYPELESS
	D24S8 = 0x4250,        // DXGI_FORMAT_R24G8_TYPELESS
	Rgba8Unknown = 0x4401, // Unknown. Zero BPP?

	// Special
	Null = 0x5100,     // Unknown
	Shadow16 = 0x5140, // DXGI_FORMAT_R16_TYPELESS
	Shadow24 = 0x5150, // DXGI_FORMAT_R24G8_TYPELESS

	// Bcn2
	Bc4Unorm = 0x6120,  // DXGI_FORMAT_BC4_UNORM
	Bc5Unorm = 0x6230,  // DXGI_FORMAT_BC5_UNORM
	Bc6hFloat = 0x6330, // DXGI_FORMAT_BC6H_SF16
	Bc7Unorm = 0x6432,  // DXGI_FORMAT_BC7_UNORM

	// Unknown7
	R16Unorm = 0x7140,  // DXGI_FORMAT_R16_UNORM
	Rg16Unorm = 0x7250, // DXGI_FORMAT_R16G16_UNORM

	// Unknown8
	R32G8 = 0x8250, // DXGI_FORMAT_R24G8_TYPELESS
}

impl Format {
	/// Texture format kind.
	pub fn kind(&self) -> FormatKind {
		FormatKind::try_from(u32::from(*self) & 0xF000).unwrap()
	}

	/// Channel or component count.
	pub fn components(&self) -> u8 {
		u8::try_from((u32::from(*self) & 0x0F00) >> 8).unwrap()
	}

	/// Bits per pixel.
	pub fn bits_per_pixel(&self) -> u8 {
		1 << ((u32::from(*self) & 0x00F0) >> 4)
	}
}

/// The overarching kind of a texture format.
#[allow(missing_docs)]
#[derive(Debug, TryFromPrimitive)]
#[repr(u32)]
pub enum FormatKind {
	Integer = 0x1000,
	Float = 0x2000,
	Bcn1 = 0x3000,
	DepthStencil = 0x4000,
	Special = 0x5000,
	Bcn2 = 0x6000,
	Unknown7 = 0x7000,
	Unknown8 = 0x8000,
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use crate::file::File;

	use super::{Texture, TextureKind};

	/// A header with no pixel data, which is all the layer count is read from.
	fn header(kind: u32, depth: u16, array_size: u8, mip_levels: u8) -> Vec<u8> {
		let mut bytes = vec![0u8; 80];
		bytes[..4].copy_from_slice(&(kind << TextureKind::SHIFT).to_le_bytes());
		bytes[4..8].copy_from_slice(&0x3420u32.to_le_bytes()); // Bc1Unorm
		bytes[8..10].copy_from_slice(&64u16.to_le_bytes());
		bytes[10..12].copy_from_slice(&64u16.to_le_bytes());
		bytes[12..14].copy_from_slice(&depth.to_le_bytes());
		bytes[14] = mip_levels;
		bytes[15] = array_size;
		bytes
	}

	fn read(bytes: Vec<u8>) -> Texture {
		Texture::read(Cursor::new(bytes)).unwrap()
	}

	/// A volume's slices halve with its width and height, so reading a level with the count from
	/// level zero walks off the end of every level below it.
	#[test]
	fn a_volume_loses_half_its_slices_each_level() {
		let texture = read(header(0b0000100, 24, 0, 6));
		assert_eq!(texture.kind(), TextureKind::D3);
		let counts = (0..6)
			.map(|level| texture.layers(level))
			.collect::<Vec<_>>();
		assert_eq!(counts, [24, 12, 6, 3, 1, 1]);
	}

	/// The other layered kinds keep every slice the whole way down.
	#[test]
	fn faces_and_array_elements_carry_down_the_chain() {
		let cube = read(header(0b0001000, 1, 0, 6));
		assert_eq!(cube.kind(), TextureKind::Cube);
		assert!((0..6).all(|level| cube.layers(level) == 6));

		let array = read(header(0b1000000, 1, 32, 6));
		assert_eq!(array.kind(), TextureKind::D2Array);
		assert!((0..6).all(|level| array.layers(level) == 32));

		let flat = read(header(0b0000010, 1, 0, 6));
		assert_eq!(flat.kind(), TextureKind::D2);
		assert!((0..6).all(|level| flat.layers(level) == 1));
	}
}
