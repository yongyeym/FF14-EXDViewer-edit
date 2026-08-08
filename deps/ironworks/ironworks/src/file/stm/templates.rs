use std::{fmt, io::Cursor};

use binrw::BinRead;
use getset::CopyGetters;
use half::f16;

use crate::{FileStream, error::Result, file::File};

use super::{invalid, structs};

/// The staining templates a material's dye table selects from.
///
/// A template holds, for every field a dye can drive, the value each stain gives that field.
pub struct StainingTemplates {
	version: u16,
	templates: Vec<Template>,
}

impl StainingTemplates {
	/// Version of the file structure, which is also what tells the two files apart.
	pub fn version(&self) -> u16 {
		self.version
	}

	/// Every template the file carries, in key order.
	pub fn templates(&self) -> &[Template] {
		&self.templates
	}

	/// The template a dye row's id names, or `None` when it belongs to the other file.
	pub fn template(&self, key: u32) -> Option<&Template> {
		self.templates.iter().find(|template| template.key == key)
	}
}

impl File for StainingTemplates {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;

		let mut cursor = Cursor::new(&bytes);
		let header = structs::Header::read(&mut cursor)?;
		// v0x0101 predates the counts being stated, and is the only file that leaves them out.
		let shape = match (header.color_count, header.scalar_count) {
			(0, 0) => (3, 2),
			(3, 9) => (3, 9),
			(colors, scalars) => {
				return Err(invalid(format!(
					"unsupported template shape of {colors} colors and {scalars} scalars"
				)));
			}
		};
		let data = usize::try_from(cursor.position()).expect("the header is small");

		let templates = header
			.keys
			.iter()
			.zip(&header.offsets)
			.map(|(&key, &offset)| Template::read(&bytes, key, data, offset, shape))
			.collect::<Result<Vec<_>>>()?;

		Ok(Self {
			version: header.version,
			templates,
		})
	}
}

impl fmt::Debug for StainingTemplates {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("StainingTemplates")
			.field("version", &format_args!("{:#06x}", self.version))
			.field("templates", &self.templates.len())
			.finish_non_exhaustive()
	}
}

/// One staining template, naming a value for each stain.
#[derive(Debug, CopyGetters)]
pub struct Template {
	/// The id a dye row carries to select this template.
	#[get_copy = "pub"]
	key: u32,

	colors: Vec<Column<[f32; 3]>>,
	scalars: Vec<Column<f32>>,
}

impl Template {
	/// How many stains a template covers. Ids run from one; zero is the unstained slot.
	pub const STAINS: usize = 254;

	/// The values a stain reads out of this template, or `None` for an id outside
	/// [`STAINS`](Self::STAINS).
	pub fn dye(&self, stain: u8) -> Option<DyePack> {
		let stain = usize::from(stain);
		if !(1..=Self::STAINS).contains(&stain) {
			return None;
		}

		let color = |index: usize| self.colors[index].value(stain);
		// A file stating two scalars carries the pre-Dawntrail pair, which Penumbra.GameData names
		// Shininess and SpecularMask and converts into these first two fields.
		let scalar = |index: usize| match self.scalars.get(index) {
			Some(column) => column.value(stain),
			None => 0.0,
		};

		Some(DyePack {
			diffuse: color(0),
			specular: color(1),
			emissive: color(2),
			scalar3: scalar(0),
			metalness: scalar(1),
			roughness: scalar(2),
			sheen_rate: scalar(3),
			sheen_tint: scalar(4),
			sheen_aperture: scalar(5),
			anisotropy: scalar(6),
			sphere_index: scalar(7) as u16,
			sphere_mask: scalar(8),
		})
	}

	fn read(
		bytes: &[u8],
		key: u32,
		data: usize,
		offset: u32,
		(colors, scalars): (usize, usize),
	) -> Result<Self> {
		let start = usize::try_from(u64::from(offset) * 2)
			.ok()
			.and_then(|offset| data.checked_add(offset))
			.filter(|start| *start <= bytes.len())
			.ok_or_else(|| invalid(format!("template {key} starts past the end of the file")))?;

		// Each column states the end it runs to, in units of two bytes from the end of this table.
		let ends = bytes
			.get(start..start + 2 * (colors + scalars))
			.ok_or_else(|| invalid(format!("template {key} is truncated")))?;
		let base = start + ends.len();

		let mut at = base;
		let columns = ends
			.chunks_exact(2)
			.map(|end| {
				let end = base + 2 * usize::from(u16::from_le_bytes([end[0], end[1]]));
				let column = bytes.get(at..end).ok_or_else(|| {
					invalid(format!("a column of template {key} runs past the file"))
				})?;
				at = end;
				Ok(column)
			})
			.collect::<Result<Vec<_>>>()?;
		let (color_columns, scalar_columns) = columns.split_at(colors);

		Ok(Self {
			key,
			colors: color_columns
				.iter()
				.map(|&column| Column::read(column, 6, half3, key))
				.collect::<Result<_>>()?,
			scalars: scalar_columns
				.iter()
				.map(|&column| Column::read(column, 2, half, key))
				.collect::<Result<_>>()?,
		})
	}
}

/// The value every dyeable field of a colour table row takes for one stain. Field meanings follow
/// Penumbra.GameData's `DyePack`; a file stating two scalars leaves everything past
/// [`metalness`](Self::metalness) at zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DyePack {
	pub diffuse: [f32; 3],
	pub specular: [f32; 3],
	pub emissive: [f32; 3],
	/// Unidentified. Takes the fourth half of the row's first vector.
	pub scalar3: f32,
	pub metalness: f32,
	pub roughness: f32,
	pub sheen_rate: f32,
	pub sheen_tint: f32,
	pub sheen_aperture: f32,
	pub anisotropy: f32,
	pub sphere_index: u16,
	pub sphere_mask: f32,
}

/// One field's value for every stain. A template holds a column per field rather than a row per
/// stain, and a column states only as much as it needs to.
#[derive(Debug)]
enum Column<T> {
	/// No value; every stain reads the default.
	Default,
	/// One value every stain reads.
	Repeated(T),
	/// One value per stain.
	Values(Vec<T>),
	/// Values shared between stains, each stain naming the one it reads.
	Indexed { values: Vec<T>, indices: Vec<u8> },
}

impl<T: Copy + Default> Column<T> {
	/// The value a stain reads, counting from one.
	fn value(&self, stain: usize) -> T {
		match self {
			Self::Default => T::default(),
			Self::Repeated(value) => *value,
			Self::Values(values) => values[stain - 1],
			// One index byte per stain at the stain's own id, leaving a byte over for the unstained
			// slot. Indices count from one, so zero selects the default.
			Self::Indexed { values, indices } => indices
				.get(stain)
				.and_then(|&index| values.get(usize::from(index).checked_sub(1)?))
				.copied()
				.unwrap_or_default(),
		}
	}

	fn read(bytes: &[u8], size: usize, read: impl Fn(&[u8]) -> T, key: u32) -> Result<Self> {
		let values = |bytes: &[u8]| bytes.chunks_exact(size).map(&read).collect();
		// A column of 254 scalars is as long as an indexed one holding 127, so the one-to-one shape
		// has to be taken first.
		Ok(match bytes.len() {
			0 => Self::Default,
			len if len == size => Self::Repeated(read(bytes)),
			len if len == Template::STAINS * size => Self::Values(values(bytes)),
			len => {
				let count = len
					.checked_sub(Template::STAINS)
					.filter(|rest| rest % size == 0)
					.ok_or_else(|| {
						invalid(format!(
							"a column of template {key} is {len} bytes, which is no shape of a {size} byte field"
						))
					})? / size;
				let (head, indices) = bytes.split_at(count * size);
				Self::Indexed {
					values: values(head),
					indices: indices.into(),
				}
			}
		})
	}
}

fn half(bytes: &[u8]) -> f32 {
	f16::from_le_bytes([bytes[0], bytes[1]]).to_f32()
}

fn half3(bytes: &[u8]) -> [f32; 3] {
	[half(bytes), half(&bytes[2..]), half(&bytes[4..])]
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use half::f16;

	use crate::{error::Error, file::File};

	use super::{StainingTemplates, Template};

	fn scalar(value: f32) -> Vec<u8> {
		f16::from_f32(value).to_le_bytes().into()
	}

	fn color(value: f32) -> Vec<u8> {
		[value, value + 1.0, value + 2.0]
			.into_iter()
			.flat_map(scalar)
			.collect()
	}

	fn one_to_one() -> Vec<u8> {
		(1..=Template::STAINS)
			.flat_map(|stain| scalar(stain as f32))
			.collect()
	}

	/// Values, the marker byte standing for the unstained slot, then one index per stain. Indices
	/// count from one, and stains past those given read the default.
	fn indexed(values: &[f32], indices: &[u8], element: fn(f32) -> Vec<u8>) -> Vec<u8> {
		let mut bytes: Vec<u8> = values.iter().copied().flat_map(element).collect();
		bytes.push(0xFF);
		bytes.extend(indices);
		bytes.resize(bytes.len() + Template::STAINS - 1 - indices.len(), 0);
		bytes
	}

	/// A template stating every one of its columns as absent.
	fn all_default(columns: usize) -> Vec<u8> {
		template(&vec![Vec::new(); columns])
	}

	fn template(columns: &[Vec<u8>]) -> Vec<u8> {
		let mut bytes = Vec::new();
		let mut end = 0;
		for column in columns {
			end += column.len();
			bytes.extend(u16::try_from(end / 2).unwrap().to_le_bytes());
		}
		bytes.extend(columns.concat());
		bytes
	}

	fn file(version: u16, shape: (u8, u8), templates: &[(u32, Vec<u8>)]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(0x534Du16.to_le_bytes());
		bytes.extend(version.to_le_bytes());
		bytes.extend(u16::try_from(templates.len()).unwrap().to_le_bytes());
		bytes.extend([shape.0, shape.1]);
		for (key, _) in templates {
			bytes.extend(key.to_le_bytes());
		}
		let mut offset = 0u32;
		for (_, body) in templates {
			bytes.extend(offset.to_le_bytes());
			offset += u32::try_from(body.len() / 2).unwrap();
		}
		for (_, body) in templates {
			bytes.extend(body);
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			StainingTemplates::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn rejects_an_unrecognised_shape() {
		let file = file(0x0301, (4, 4), &[(100, all_default(8))]);
		assert!(matches!(
			StainingTemplates::read(Cursor::new(file)),
			Err(Error::Invalid(..))
		));
	}

	/// A file stating no columns carries the pre-Dawntrail pair of scalars.
	#[test]
	fn reads_every_column_shape() {
		let bytes = file(
			0x0101,
			(0, 0),
			&[
				(
					100,
					template(&[
						vec![],
						color(1.0),
						indexed(&[10.0, 20.0], &[2, 1, 0], color),
						one_to_one(),
						indexed(&[7.0], &[1], scalar),
					]),
				),
				(101, all_default(5)),
			],
		);
		let file = StainingTemplates::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.version(), 0x0101);
		assert_eq!(file.templates().len(), 2);
		assert!(file.template(102).is_none());
		assert_eq!(
			file.template(101).unwrap().dye(1).unwrap().specular,
			[0.0; 3]
		);

		let template = file.template(100).unwrap();
		let first = template.dye(1).unwrap();
		assert_eq!(first.diffuse, [0.0; 3]);
		assert_eq!(first.specular, [1.0, 2.0, 3.0]);
		assert_eq!(first.emissive, [20.0, 21.0, 22.0]);
		assert_eq!(first.scalar3, 1.0);
		assert_eq!(first.metalness, 7.0);
		assert_eq!(template.dye(2).unwrap().emissive, [10.0, 11.0, 12.0]);
		assert_eq!(template.dye(2).unwrap().metalness, 0.0);
		assert_eq!(template.dye(3).unwrap().emissive, [0.0; 3]);
		// The marker byte leaves an indexed column one stain short of the others.
		assert_eq!(template.dye(254).unwrap().scalar3, 254.0);
		assert_eq!(template.dye(254).unwrap().emissive, [0.0; 3]);
		assert!(template.dye(0).is_none());
		assert!(template.dye(255).is_none());
	}

	/// A file stating two scalars leaves the seven Dawntrail added at zero.
	#[test]
	fn leaves_scalars_a_legacy_file_does_not_carry() {
		let bytes = file(0x0101, (0, 0), &[(100, all_default(5))]);
		let file = StainingTemplates::read(Cursor::new(bytes)).unwrap();
		let dye = file.template(100).unwrap().dye(1).unwrap();
		assert_eq!(dye.roughness, 0.0);
		assert_eq!(dye.sphere_index, 0);
	}

	#[test]
	fn reads_the_scalars_in_order() {
		let columns = vec![Vec::new(); 3]
			.into_iter()
			.chain((1..=9).map(|value| scalar(value as f32)))
			.collect::<Vec<_>>();
		let bytes = file(0x0201, (3, 9), &[(1100, template(&columns))]);
		let file = StainingTemplates::read(Cursor::new(bytes)).unwrap();
		let dye = file.template(1100).unwrap().dye(1).unwrap();
		assert_eq!(dye.scalar3, 1.0);
		assert_eq!(dye.metalness, 2.0);
		assert_eq!(dye.roughness, 3.0);
		assert_eq!(dye.sheen_rate, 4.0);
		assert_eq!(dye.sheen_tint, 5.0);
		assert_eq!(dye.sheen_aperture, 6.0);
		assert_eq!(dye.anisotropy, 7.0);
		assert_eq!(dye.sphere_index, 8);
		assert_eq!(dye.sphere_mask, 9.0);
	}
}
