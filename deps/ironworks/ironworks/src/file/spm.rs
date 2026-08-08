//! Structs and utilities for parsing .spm files.

use std::io::{Cursor, SeekFrom};

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// The table of parameters a shader indexes by the profile a material names.
///
/// Three of these ship, one each for `bg`, `chara` and `common`, in `common/graphics`. Every one is
/// a grid: the columns are the parameters, the rows are the profiles, and each cell is one word
/// read as whatever its column says. Every one is at version `0x01000000`.
#[binread]
#[br(little, magic = 0x0100_0000u32)]
#[derive(Debug, CopyGetters)]
pub struct ShaderParameters {
	#[br(temp)]
	column_count: u8,

	#[br(temp)]
	row_count: u8,

	// Offsets are counted in words from the start of the file.
	#[br(temp)]
	columns_offset: u16,

	#[br(temp)]
	rows_offset: u16,

	#[br(temp)]
	values_offset: u16,

	#[br(seek_before = at(columns_offset), count = column_count)]
	#[getset(skip)]
	columns: Vec<Column>,

	#[br(seek_before = at(rows_offset), count = row_count)]
	#[getset(skip)]
	rows: Vec<Row>,

	#[br(
		seek_before = at(values_offset),
		count = usize::from(row_count) * usize::from(column_count),
	)]
	#[getset(skip)]
	values: Vec<u32>,
}

fn at(offset: u16) -> SeekFrom {
	SeekFrom::Start(u64::from(offset) * 4)
}

impl ShaderParameters {
	/// The parameters the table holds, which are its columns.
	pub fn columns(&self) -> &[Column] {
		&self.columns
	}

	/// The profiles the table holds, which are its rows.
	pub fn rows(&self) -> &[Row] {
		&self.rows
	}

	/// What one profile sets one parameter to.
	pub fn value(&self, row: usize, column: usize) -> Option<Value> {
		let index = row.checked_mul(self.columns.len())? + column;
		let raw = *self.values.get(index)?;
		Some(match self.columns.get(column)?.kind {
			Kind::Float => Value::Float(f32::from_bits(raw)),
			Kind::Unsigned => Value::Unsigned(raw),
			Kind::Name => Value::Name(raw),
		})
	}
}

impl File for ShaderParameters {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;
		Ok(<Self as BinRead>::read(&mut Cursor::new(bytes))?)
	}
}

/// One parameter the table carries.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Column {
	/// Hashed name of the parameter, which [`name`] resolves where it is one of the known ones.
	id: u32,

	/// How the cells of this column are read.
	kind: Kind,
}

/// How a cell is read.
#[binread]
#[br(little, repr = u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	Float = 0,
	Unsigned = 1,

	/// Another hashed name, resolved the same way as a column's.
	Name = 2,
}

/// One profile the table carries.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Row {
	/// Hashed name of the table the profile belongs to, which is the same for every row of a file.
	table: u32,

	/// What a material names to select the profile.
	index: u32,
}

/// One cell of the table, read as its column says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
	Float(f32),
	Unsigned(u32),

	/// A hashed name, resolved by [`name`].
	Name(u32),
}

/// The name behind one of the hashes the format writes, over the parameters, the tables and the
/// values that are themselves names. Nothing in the file spells them out; these are the ones that
/// have been recognised.
pub fn name(id: u32) -> Option<&'static str> {
	Some(match id {
		0xE800_1A59 => "LightingType",
		0x8FB5_3404 => "SubSurfaceProfileID",
		0xF30D_1232 => "SubSurfaceWidth",
		0x41338E94 => "BackScatterPower",
		0xB1FE_BD21 => "SheenRate",
		0xB786_7D05 => "SheenTintRate",
		0x5C30_C2FC => "SheenAperture",
		0x9472_05D5 => "UseSubSurfaceRate",
		0x671C_995B => "HairScatterColorShift",
		0x6CD8_77F3 => "HairSpecularShift",
		0x85DC_1E5C => "FurLength",
		0x8F6B_A743 => "HairRoughnessOffsetRate",
		0xD49D_56BD => "SubSurfacePower",
		0x4D31_0CC0 => "Reserve",
		0xF33F_F064 => "HairSpecularPrimaryShift",
		0xE0D2_4CB4 => "HairSpecularSecondaryShift",
		0xA46A_47BB => "HairSpecularBackScatterShift",
		0x773A_D7FB => "HairBackScatterRoughnessOffsetRate",
		0xA8C9_9005 => "HairSecondaryRoughnessOffsetRate",

		0x2AF7_F9B4 => "BG",
		0xB9FD_FB6C => "CHARA",
		0xA4D6_1674 => "COMMON",

		0x4177_21BB => "DEFAULT",
		0x56F1_6FCB => "LEGACY",

		_ => return None,
	})
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Kind, ShaderParameters, Value, name};

	/// A table of `columns` by `rows`, with the three blocks written in an order the header alone
	/// resolves, so a reader following the file rather than the offsets lands in the wrong one.
	fn parameters(columns: &[(u32, Kind)], rows: &[(u32, u32)]) -> Vec<u8> {
		let values_offset = 3u16;
		let rows_offset = values_offset + u16::try_from(columns.len() * rows.len()).unwrap();
		let columns_offset = rows_offset + u16::try_from(rows.len() * 2).unwrap();

		let mut bytes = Vec::new();
		bytes.extend(0x0100_0000u32.to_le_bytes());
		bytes.push(u8::try_from(columns.len()).unwrap());
		bytes.push(u8::try_from(rows.len()).unwrap());
		bytes.extend(columns_offset.to_le_bytes());
		bytes.extend(rows_offset.to_le_bytes());
		bytes.extend(values_offset.to_le_bytes());

		for row in 0..rows.len() {
			for (column, (_, kind)) in columns.iter().enumerate() {
				let raw = (row * 10 + column) as u32;
				bytes.extend(
					match kind {
						Kind::Float => (raw as f32).to_bits(),
						_ => raw,
					}
					.to_le_bytes(),
				);
			}
		}
		for (table, index) in rows {
			bytes.extend(table.to_le_bytes());
			bytes.extend(index.to_le_bytes());
		}
		for (id, kind) in columns {
			bytes.extend(id.to_le_bytes());
			bytes.extend((*kind as u32).to_le_bytes());
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			ShaderParameters::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_each_block_from_its_own_offset() {
		let file = ShaderParameters::read(Cursor::new(parameters(
			&[
				(0x8FB5_3404, Kind::Unsigned),
				(0xF30D_1232, Kind::Float),
				(0xE800_1A59, Kind::Name),
			],
			&[(0xB9FD_FB6C, 0), (0xB9FD_FB6C, 7)],
		)))
		.unwrap();

		let columns = file.columns();
		assert_eq!(columns.len(), 3);
		assert_eq!(name(columns[0].id()), Some("SubSurfaceProfileID"));
		assert_eq!(columns[2].kind(), Kind::Name);

		let rows = file.rows();
		assert_eq!(rows.len(), 2);
		assert_eq!(name(rows[1].table()), Some("CHARA"));
		assert_eq!(rows[1].index(), 7);
	}

	/// A cell is read as the kind its column gives, not as the bits alone.
	#[test]
	fn reads_a_cell_as_its_column_says() {
		let file = ShaderParameters::read(Cursor::new(parameters(
			&[(0x8FB5_3404, Kind::Unsigned), (0xF30D_1232, Kind::Float)],
			&[(0x2AF7_F9B4, 0), (0x2AF7_F9B4, 1)],
		)))
		.unwrap();

		assert_eq!(file.value(0, 0), Some(Value::Unsigned(0)));
		assert_eq!(file.value(0, 1), Some(Value::Float(1.0)));
		assert_eq!(file.value(1, 0), Some(Value::Unsigned(10)));
		assert_eq!(file.value(1, 1), Some(Value::Float(11.0)));
		assert_eq!(file.value(2, 0), None);
		assert_eq!(file.value(0, 2), None);
	}

	#[test]
	fn a_hash_the_format_does_not_name() {
		assert_eq!(name(0x8B26_53B1), None);
	}

	#[test]
	fn rejects_another_version() {
		let mut bytes = parameters(&[(0, Kind::Float)], &[(0, 0)]);
		bytes[3] = 2;
		assert!(matches!(
			ShaderParameters::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
