use std::io::{Read, Seek, SeekFrom};

use binrw::{BinRead, BinResult, Endian, helpers::count};
use getset::{CopyGetters, Getters};

/// Read the `hpla` block of animation layers, which starts at the reader's position.
pub fn layers<R: Read + Seek>(
	reader: &mut R,
	options: Endian,
	_args: (),
) -> BinResult<Vec<AnimationLayer>> {
	let base_offset = reader.stream_position()?;

	let magic = <[u8; 4]>::read(reader)?;
	if &magic != b"hpla" {
		return Err(binrw::Error::BadMagic {
			pos: base_offset,
			found: Box::new(magic),
		});
	}

	let layer_count = u16::read_le(reader)?;
	count(layer_count.into())(reader, options, (base_offset,))
}

/// One animation layer, and the bones it drives.
#[derive(Debug, Getters, CopyGetters)]
pub struct AnimationLayer {
	///
	#[get_copy = "pub"]
	layer: u32,

	///
	#[get = "pub"]
	bone_indices: Vec<i16>,
}

impl BinRead for AnimationLayer {
	type Args<'a> = (u64,);

	fn read_options<R: Read + Seek>(
		reader: &mut R,
		options: Endian,
		(base_offset,): Self::Args<'_>,
	) -> BinResult<Self> {
		let offset = u16::read_le(reader)?;
		let position = reader.stream_position()?;

		reader.seek(SeekFrom::Start(base_offset + u64::from(offset)))?;

		let layer = u32::read_le(reader)?;
		let bone_count = u16::read_le(reader)?;
		let bone_indices = count(bone_count.into())(reader, options, ())?;

		let result = Self {
			layer,
			bone_indices,
		};

		reader.seek(SeekFrom::Start(position))?;

		Ok(result)
	}
}
