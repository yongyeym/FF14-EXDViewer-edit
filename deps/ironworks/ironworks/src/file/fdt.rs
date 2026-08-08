//! Structs and utilities for parsing .fdt files.

use std::io::Cursor;

use binrw::{BinRead, binread, meta::ReadEndian};
use getset::CopyGetters;

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;

/// A font: the metrics of every character it draws, and where each is cut from the font textures.
///
/// The glyphs themselves live in `common/font/font*.tex`, four fonts to a texture -- one per colour
/// channel -- which is what [`Glyph::texture_file`] and [`Glyph::texture_channel`] split apart.
#[derive(Debug, CopyGetters)]
pub struct FontData {
	/// Size of the textures the glyphs were baked into.
	#[get_copy = "pub"]
	texture_width: u16,

	#[get_copy = "pub"]
	texture_height: u16,

	/// Point size the font was baked at.
	#[get_copy = "pub"]
	size: f32,

	/// Distance between the baselines of consecutive lines.
	#[get_copy = "pub"]
	line_height: i32,

	/// Distance from the top of a line to its baseline.
	#[get_copy = "pub"]
	ascent: i32,

	glyphs: Vec<Glyph>,

	kerning: Vec<Kerning>,
}

impl FontData {
	/// Every glyph the font draws, ordered by character but not strictly: the AXIS fonts carry two
	/// entries for the space, cut from different places in the sheet.
	pub fn glyphs(&self) -> &[Glyph] {
		&self.glyphs
	}

	/// The glyph for a character, or `None` where the font does not draw it.
	pub fn glyph(&self, character: char) -> Option<&Glyph> {
		let index = self
			.glyphs
			.binary_search_by_key(&character, Glyph::character)
			.ok()?;
		self.glyphs.get(index)
	}

	/// Every pair the font moves closer together or further apart, ordered by character.
	pub fn kerning(&self) -> &[Kerning] {
		&self.kerning
	}

	/// How far to move `right` when it follows `left`, which is zero for most pairs.
	pub fn kerning_between(&self, left: char, right: char) -> i32 {
		self.kerning
			.binary_search_by_key(&(left, right), |pair| (pair.left, pair.right))
			.map_or(0, |index| self.kerning[index].offset)
	}

	/// Distance from a baseline to the bottom of its line.
	pub fn descent(&self) -> i32 {
		self.line_height - self.ascent
	}
}

/// One character, as a rectangle of a font texture plus what it does to the pen.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Glyph {
	/// The character drawn.
	#[br(map = character)]
	character: char,

	/// The same character in Shift JIS, which is how the game indexed fonts before it used Unicode.
	shift_jis: u16,

	/// Which font texture and channel the glyph was baked into. [`texture_file`](Self::texture_file)
	/// and [`texture_channel`](Self::texture_channel) are the halves of it.
	texture_index: u16,

	/// Position within the texture, in pixels.
	x: u16,
	y: u16,

	/// Size of the inked rectangle, in pixels.
	width: u8,
	height: u8,

	/// Gap between this glyph's rectangle and the next glyph's, which is negative where they
	/// overlap.
	next_offset_x: i8,

	/// How far below the top of the line the rectangle is drawn.
	offset_y: i8,
}

impl Glyph {
	/// Which of the font's textures holds the glyph, counting from zero.
	pub fn texture_file(&self) -> u16 {
		self.texture_index / 4
	}

	/// Which colour channel of that texture holds it, in RGBA order.
	pub fn texture_channel(&self) -> u16 {
		self.texture_index % 4
	}

	/// How far the pen moves after drawing this glyph.
	pub fn advance_width(&self) -> i32 {
		i32::from(self.width) + i32::from(self.next_offset_x)
	}
}

/// How far a pair of characters moves when they are drawn together.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Kerning {
	#[br(map = character)]
	left: char,
	#[br(map = character)]
	right: char,

	left_shift_jis: u16,
	right_shift_jis: u16,

	/// Added to the pen after the left character, so a negative value tucks the pair together.
	offset: i32,
}

impl File for FontData {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		Self::parse(&bytes)
	}
}

impl FontData {
	fn parse(bytes: &[u8]) -> Result<Self> {
		let header = structs::Header::read(&mut cursor(bytes, 0)?)?;
		let font_at = usize::try_from(header.font_table_offset).expect("u32 fits usize");
		let kerning_at = usize::try_from(header.kerning_table_offset).expect("u32 fits usize");

		let font = structs::FontTable::read(&mut cursor(bytes, font_at)?)?;
		let glyphs = table(bytes, font_at + structs::FontTable::SIZE, font.glyph_count)?;

		let pairs = structs::KerningTable::read(&mut cursor(bytes, kerning_at)?)?;
		let kerning = table(bytes, kerning_at + structs::KerningTable::SIZE, pairs.count)?;

		Ok(Self {
			texture_width: font.texture_width,
			texture_height: font.texture_height,
			size: font.size,
			line_height: font.line_height,
			ascent: font.ascent,
			glyphs,
			kerning,
		})
	}
}

/// Bytes every record in both tables takes.
const RECORD: usize = 16;

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("FDT".into()), reason.into())
}

/// A cursor over the file from `at` onwards.
fn cursor(bytes: &[u8], at: usize) -> Result<Cursor<&[u8]>> {
	match bytes.get(at..) {
		Some(rest) => Ok(Cursor::new(rest)),
		None => Err(invalid(format!(
			"offset {at:#x} is past the end of the file"
		))),
	}
}

/// Read `count` records of 16 bytes each, starting at `at`.
///
/// Both tables are read from the offset their header gives rather than from where the previous one
/// ended: the Chinese fonts leave sixteen bytes between them.
fn table<T>(bytes: &[u8], at: usize, count: u32) -> Result<Vec<T>>
where
	T: for<'a> BinRead<Args<'a> = ()> + ReadEndian,
{
	let count = usize::try_from(count).expect("u32 fits usize");
	// Slicing to the table's own extent rather than to the end of the file both bounds-checks the
	// count and stops a record reading past the last one.
	let records = count
		.checked_mul(RECORD)
		.and_then(|size| at.checked_add(size))
		.and_then(|end| bytes.get(at..end))
		.ok_or_else(|| {
			invalid(format!(
				"table at {at:#x} declares {count} records, which do not fit in the file"
			))
		})?;

	let mut cursor = Cursor::new(records);
	(0..count).map(|_| Ok(T::read(&mut cursor)?)).collect()
}

/// The character whose UTF-8 the file packed into a word, most significant byte first. Anything the
/// file wrote badly comes back as [`char::REPLACEMENT_CHARACTER`].
fn character(packed: u32) -> char {
	let bytes = packed.to_be_bytes();
	let leading = bytes.iter().take_while(|byte| **byte == 0).count();
	std::str::from_utf8(&bytes[leading.min(3)..])
		.ok()
		.and_then(|text| text.chars().next())
		.unwrap_or(char::REPLACEMENT_CHARACTER)
}

mod structs {
	use binrw::binread;

	/// The file's root header.
	#[binread]
	#[br(little, magic = b"fcsv")]
	#[derive(Debug)]
	pub struct Header {
		/// "0100" in every font the game ships.
		_version: [u8; 4],
		pub font_table_offset: u32,
		#[br(pad_after = 0x10)]
		pub kerning_table_offset: u32,
	}

	/// The glyph table's header, which the glyphs follow immediately.
	#[binread]
	#[br(little, magic = b"fthd")]
	#[derive(Debug)]
	pub struct FontTable {
		pub glyph_count: u32,
		/// Repeated by the kerning table's own header, which is what ironworks reads instead.
		_kerning_count: u32,
		#[br(pad_before = 4)]
		pub texture_width: u16,
		pub texture_height: u16,
		pub size: f32,
		pub line_height: i32,
		pub ascent: i32,
	}

	impl FontTable {
		pub const SIZE: usize = 32;
	}

	#[binread]
	#[br(little, magic = b"knhd")]
	#[derive(Debug)]
	pub struct KerningTable {
		#[br(pad_after = 8)]
		pub count: u32,
	}

	impl KerningTable {
		pub const SIZE: usize = 16;
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{FontData, character};

	/// A file with one glyph and one kerning pair, with `gap` bytes left between the two tables.
	fn font(gap: usize) -> Vec<u8> {
		let font_at = 32u32;
		let kerning_at = font_at + 32 + 16 + gap as u32;

		let mut bytes = Vec::new();
		bytes.extend(b"fcsv0100");
		bytes.extend(font_at.to_le_bytes());
		bytes.extend(kerning_at.to_le_bytes());
		bytes.extend([0; 0x10]);

		bytes.extend(b"fthd");
		bytes.extend(1u32.to_le_bytes());
		bytes.extend(1u32.to_le_bytes());
		bytes.extend([0; 4]);
		bytes.extend(1024u16.to_le_bytes());
		bytes.extend(512u16.to_le_bytes());
		bytes.extend(36.0f32.to_le_bytes());
		bytes.extend(48i32.to_le_bytes());
		bytes.extend(38i32.to_le_bytes());

		// One glyph: 'A' at 470,471, 10x16, one pixel of overlap with whatever follows it.
		bytes.extend(0x41u32.to_le_bytes());
		bytes.extend(0x41u16.to_le_bytes());
		bytes.extend(9u16.to_le_bytes());
		bytes.extend(470u16.to_le_bytes());
		bytes.extend(471u16.to_le_bytes());
		bytes.extend([10, 16]);
		bytes.extend((-1i8).to_le_bytes());
		bytes.extend(2i8.to_le_bytes());

		bytes.extend(vec![0xCC; gap]);

		bytes.extend(b"knhd");
		bytes.extend(1u32.to_le_bytes());
		bytes.extend([0; 8]);

		bytes.extend(0x41u32.to_le_bytes());
		bytes.extend(0x56u32.to_le_bytes());
		bytes.extend(0x41u16.to_le_bytes());
		bytes.extend(0x56u16.to_le_bytes());
		bytes.extend((-2i32).to_le_bytes());
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			FontData::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_a_glyph_and_its_metrics() {
		let font = FontData::read(Cursor::new(font(0))).unwrap();
		assert_eq!((font.texture_width(), font.texture_height()), (1024, 512));
		assert_eq!(font.size(), 36.0);
		assert_eq!(
			(font.line_height(), font.ascent(), font.descent()),
			(48, 38, 10)
		);

		let glyph = font.glyph('A').expect("'A' is in the font");
		assert_eq!((glyph.x(), glyph.y()), (470, 471));
		assert_eq!((glyph.width(), glyph.height()), (10, 16));
		assert_eq!((glyph.texture_file(), glyph.texture_channel()), (2, 1));
		assert_eq!(glyph.advance_width(), 9);
		assert_eq!(glyph.offset_y(), 2);
		assert!(font.glyph('B').is_none());

		assert_eq!(font.kerning_between('A', 'V'), -2);
		assert_eq!(font.kerning_between('V', 'A'), 0);
	}

	/// The Chinese fonts leave sixteen bytes between the glyph and kerning tables, so the kerning
	/// header has to be found by its own offset rather than by where the glyphs ended.
	#[test]
	fn a_gap_between_the_tables_does_not_desync() {
		let font = FontData::read(Cursor::new(font(16))).unwrap();
		assert_eq!(font.glyphs().len(), 1);
		assert_eq!(font.kerning().len(), 1);
		assert_eq!(font.kerning()[0].offset(), -2);
	}

	#[test]
	fn a_table_running_past_the_end_is_an_error() {
		let mut bytes = font(0);
		bytes.truncate(bytes.len() - 8);
		assert!(matches!(
			FontData::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn characters_unpack_from_utf8() {
		assert_eq!(character(0x41), 'A');
		assert_eq!(character(0xC2A9), '©');
		assert_eq!(character(0xE6BB82), '滂');
		assert_eq!(character(0xF09F9880), '😀');
		assert_eq!(character(0), '\0');
		assert_eq!(character(0xFF), char::REPLACEMENT_CHARACTER);
		assert_eq!(character(0xEDA080), char::REPLACEMENT_CHARACTER);
	}
}
