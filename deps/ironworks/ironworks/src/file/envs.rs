//! Structs and utilities shared by the environment set formats.
//!
//! `.envb`, `.obsb` and `.essb` each wrap one `ENVS` section, which holds a timeline per weather.
//! A timeline is split into sets, each animating one thing over the day from its own keyframes.
//!
//! What a set animates decides how its keyframes are laid out. Those layouts, and the names of
//! everything inside them, come from pmgr's `env.hexpat`, which is the only place they are written
//! down.

use getset::{CopyGetters, Getters};

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

/// The file header, ahead of the section.
const HEADER: usize = 0x0C;

/// Bytes one weather takes.
const WEATHER: usize = 16;

/// Bytes one set takes.
const SET: usize = 12;

/// Marks the fields a keyframe grew, written after the ones it already had.
const EXTENDED: u32 = u32::from_le_bytes(*b"007V");

/// Bytes the marker itself takes, ahead of the fields it introduces.
const MARKER: usize = 4;

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("environment set".into()), reason.into())
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
	bytes
		.get(at..at + 4)
		.and_then(|raw| raw.try_into().ok())
		.map(u32::from_le_bytes)
		.ok_or_else(|| invalid(format!("offset {at:#x} is past the end of the file")))
}

fn f32_at(bytes: &[u8], at: usize) -> Result<f32> {
	u32_at(bytes, at).map(f32::from_bits)
}

fn byte_at(bytes: &[u8], at: usize) -> Result<u8> {
	bytes
		.get(at)
		.copied()
		.ok_or_else(|| invalid(format!("offset {at:#x} is past the end of the file")))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
	bytes
		.get(at..at + 2)
		.and_then(|raw| raw.try_into().ok())
		.map(u16::from_le_bytes)
		.ok_or_else(|| invalid(format!("offset {at:#x} is past the end of the file")))
}

/// Where the offset at `field` reaches, which the format writes signed from `base` and may point
/// backwards.
fn seek(bytes: &[u8], base: usize, field: usize) -> Result<usize> {
	let offset = u32_at(bytes, field)? as i32;
	base.checked_add_signed(offset as isize)
		.filter(|&at| at < bytes.len())
		.ok_or_else(|| invalid(format!("offset {offset} from {base:#x} leaves the file")))
}

/// A count the file states, rejected where the bytes behind the table cannot hold that many:
/// collecting a range reserves the whole count before the first read fails.
fn entries(bytes: &[u8], declared: u32, table: usize, stride: usize) -> Result<usize> {
	let count = declared as usize;
	let room = bytes.len().saturating_sub(table) / stride;
	match count <= room {
		true => Ok(count),
		false => Err(invalid(format!("a count of {count} in {room} entries"))),
	}
}

/// A null-terminated string at `at`, or an empty one where the offset does not name a string.
fn string(bytes: &[u8], at: usize) -> String {
	let Some(rest) = bytes.get(at..) else {
		return String::new();
	};
	let end = rest
		.iter()
		.position(|&byte| byte == 0)
		.unwrap_or(rest.len());
	String::from_utf8_lossy(&rest[..end]).into_owned()
}

/// Everything an `ENVS` section holds.
#[derive(Debug, Getters, CopyGetters)]
pub struct Environments {
	/// The client reads `.envb` at version 6, and the other two at version 4.
	#[get_copy = "pub"]
	version: u32,

	/// Twelve switches, named by [`OPTIONS`] and set only by `.obsb`.
	#[get_copy = "pub"]
	options: [bool; OPTIONS.len()],

	#[getset(skip)]
	weathers: Vec<Weather>,
}

impl Environments {
	/// The weathers the set covers, in the order it names them.
	pub fn weathers(&self) -> &[Weather] {
		&self.weathers
	}
}

/// What each of an [`Environments`] switch turns on. All of them steer the oscillator an
/// `ObjectOscillator` set drives, or what it is applied to.
pub const OPTIONS: [&str; 12] = [
	"Random oscillator waveform",
	"Visibility oscillation",
	"Rotation oscillation",
	"Transform rate",
	"Modulate RGB color",
	"Modulate first color",
	"Modulate RGBA color",
	"Oscillator sync",
	"Multiply tint",
	"First color over white",
	"Modulate second color",
	"Second color over white",
];

/// One weather's worth of settings.
#[derive(Debug, Getters, CopyGetters)]
pub struct Weather {
	/// A row of `Weather`.
	#[get_copy = "pub"]
	id: u32,

	/// Seconds the sets run over before they repeat.
	#[get_copy = "pub"]
	length: f32,

	#[get_copy = "pub"]
	parameter: u32,

	#[get_copy = "pub"]
	weight: f32,

	/// Two assets the weather names, which almost every file the game ships leaves empty.
	#[get = "pub"]
	paths: [String; 2],

	#[getset(skip)]
	sets: Vec<Set>,
}

impl Weather {
	/// What the weather animates, in the order it names them.
	pub fn sets(&self) -> &[Set] {
		&self.sets
	}

	fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		let table = seek(bytes, at, at)?;
		let count = entries(bytes, u32_at(bytes, at + 4)?, table, SET)?;
		let footer = seek(bytes, at, at + 12)?;

		Ok(Self {
			id: u32_at(bytes, at + 8)?,
			length: f32_at(bytes, footer)?,
			parameter: u32_at(bytes, footer + 4)?,
			weight: f32_at(bytes, footer + 8)?,
			paths: [
				seek(bytes, footer, footer + 12)?,
				seek(bytes, footer, footer + 16)?,
			]
			.map(|at| string(bytes, at)),
			sets: (0..count)
				.map(|index| Set::parse(bytes, table + index * SET))
				.collect::<Result<_>>()?,
		})
	}
}

/// One thing a weather animates.
#[derive(Debug, CopyGetters)]
pub struct Set {
	/// What the set animates, which each of the three formats numbers in its own range. It also
	/// says how the rest of a keyframe is laid out.
	#[get_copy = "pub"]
	kind: u32,

	keyframes: Vec<Keyframe>,
}

impl Set {
	/// The keyframes of the set, ascending by time.
	pub fn keyframes(&self) -> &[Keyframe] {
		&self.keyframes
	}

	/// What the set animates, spelled out.
	pub fn name(&self) -> Option<&'static str> {
		name(self.kind)
	}

	fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		let kind = u32_at(bytes, at + 8)?;
		let layout = layout(kind).ok_or_else(|| invalid(format!("unknown kind {kind}")))?;
		let table = seek(bytes, at, at)?;
		let count = entries(bytes, u32_at(bytes, at + 4)?, table, size_of::<i32>())?;

		Ok(Self {
			kind,
			keyframes: (0..count)
				.map(|index| {
					Keyframe::parse(bytes, seek(bytes, table, table + index * 4)?, &layout)
				})
				.collect::<Result<_>>()?,
		})
	}
}

/// What a set of each kind animates.
fn name(kind: u32) -> Option<&'static str> {
	Some(match kind {
		0 => "Global lighting",
		1 => "Fake specular",
		2 => "Cloud",
		3 => "Rain",
		4 => "Snow",
		5 => "Dust",
		6 => "Wind",
		7 => "Light shaft",
		8 => "Wetness",
		9 => "Tone mapping",
		10 => "Color filter",
		11 => "Effect",
		12 => "Starfield",
		13 => "Vertical fog",
		20 => "Ambient sound paths",
		21 => "Ambient sound flags",
		29 => "Object visibility",
		30 => "Object transform",
		31 => "Object oscillator",
		32 => "Object rotation",
		33 => "Object RGB color",
		34 => "Object RGB color pair",
		35 => "Object RGBA color",
		_ => return None,
	})
}

/// One point on a set's timeline.
#[derive(Debug, CopyGetters)]
pub struct Keyframe {
	/// Seconds since midnight.
	#[get_copy = "pub"]
	time: f32,

	#[getset(skip)]
	fields: Vec<(&'static str, Value)>,
}

impl Keyframe {
	/// What the keyframe sets, named and in the order the format writes them. Which fields a
	/// keyframe carries follows the kind of its set.
	pub fn fields(&self) -> &[(&'static str, Value)] {
		&self.fields
	}

	/// The colours the keyframe reaches, in the order it names them.
	pub fn colours(&self) -> impl Iterator<Item = Colour> {
		self.fields.iter().filter_map(|(_, value)| match value {
			Value::Colour(colour) => Some(*colour),
			_ => None,
		})
	}

	/// The assets the keyframe names, which `.envb` uses for effects and `.essb` for sound.
	pub fn paths(&self) -> impl Iterator<Item = &str> {
		self.fields.iter().filter_map(|(_, value)| match value {
			Value::Path(path) => Some(path.as_str()),
			_ => None,
		})
	}

	fn parse(bytes: &[u8], at: usize, layout: &Layout) -> Result<Self> {
		let mut fields = Vec::new();
		values(bytes, at, at + 4, layout.fields, layout.paths, &mut fields)?;

		let extension = at + layout.size();
		if !layout.extension.is_empty() && u32_at(bytes, extension).is_ok_and(|it| it == EXTENDED) {
			values(
				bytes,
				at,
				extension + MARKER,
				layout.extension,
				&[],
				&mut fields,
			)?;
		}

		Ok(Self {
			time: f32_at(bytes, at)?,
			fields,
		})
	}
}

/// Reads `fields` starting at `from`, resolving colour and path offsets against `base`, which is
/// the start of the keyframe however far into it the offset itself sits.
fn values(
	bytes: &[u8],
	base: usize,
	from: usize,
	fields: &[Field],
	paths: &[&'static str],
	into: &mut Vec<(&'static str, Value)>,
) -> Result<()> {
	let mut at = from;
	for field in fields {
		match field {
			Field::Float(name) => into.push((name, Value::Float(f32_at(bytes, at)?))),
			Field::Unsigned(name) => into.push((name, Value::Unsigned(u32_at(bytes, at)?))),
			Field::Short(name) => {
				into.push((name, Value::Unsigned(u16_at(bytes, at)?.into())));
			}
			Field::Byte(name) => into.push((name, Value::Unsigned(byte_at(bytes, at)?.into()))),
			Field::Flag(name) => into.push((name, Value::Flag(byte_at(bytes, at)? != 0))),
			Field::Colour(name) => {
				let colour = Colour::parse(bytes, seek(bytes, base, at)?)?;
				into.push((name, Value::Colour(colour)));
			}
			Field::Paths => {
				let table = seek(bytes, base, at)?;
				let count = entries(bytes, u32_at(bytes, at + 4)?, table, size_of::<i32>())?;
				for index in 0..count {
					let path = string(bytes, seek(bytes, table, table + index * 4)?);
					into.push((
						paths.get(index).copied().unwrap_or_default(),
						Value::Path(path),
					));
				}
			}
			Field::Padding(_) => (),
		}
		at += field.size();
	}
	Ok(())
}

/// What one field of a keyframe holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	Float(f32),
	Unsigned(u32),
	Flag(bool),
	Colour(Colour),
	Path(String),
}

/// A colour and the intensity it is scaled by.
#[derive(Debug, Clone, Copy, PartialEq, CopyGetters)]
#[get_copy = "pub"]
pub struct Colour {
	red: u8,
	green: u8,
	blue: u8,
	alpha: u8,
	intensity: f32,
}

impl Colour {
	fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		let [red, green, blue, alpha] = u32_at(bytes, at)?.to_le_bytes();
		Ok(Self {
			red,
			green,
			blue,
			alpha,
			intensity: f32_at(bytes, at + 4)?,
		})
	}
}

/// One field of a keyframe: what it is called and how it is read.
enum Field {
	Float(&'static str),
	Unsigned(&'static str),
	Short(&'static str),
	Byte(&'static str),
	Flag(&'static str),

	/// A signed offset from the start of the keyframe, reaching a colour.
	Colour(&'static str),

	/// The offset and count of the keyframe's path table, whose entries its layout names.
	Paths,

	/// Bytes written only to align what follows.
	Padding(usize),
}

impl Field {
	fn size(&self) -> usize {
		match self {
			Self::Float(_) | Self::Unsigned(_) | Self::Colour(_) => 4,
			Self::Short(_) => 2,
			Self::Byte(_) | Self::Flag(_) => 1,
			Self::Paths => 8,
			Self::Padding(size) => *size,
		}
	}
}

/// How the keyframes of one kind are laid out: the fields past the time they open with, the names
/// of the assets their path table holds, and the fields they grew.
struct Layout {
	fields: &'static [Field],
	paths: &'static [&'static str],
	extension: &'static [Field],
}

impl Layout {
	/// Bytes a keyframe takes before the fields it grew, the time included.
	fn size(&self) -> usize {
		4 + self.fields.iter().map(Field::size).sum::<usize>()
	}
}

use Field::{Byte, Colour as Rgba, Flag, Float, Padding, Paths, Short, Unsigned};

#[rustfmt::skip]
fn layout(kind: u32) -> Option<Layout> {
	let (fields, paths, extension): (&[Field], &[&str], &[Field]) = match kind {
		0 => (&[
			Rgba("sunlight_color"), Float("ambient_light_scale"), Float("ambient_light_saturation"),
			Float("ambient_attenuation"), Rgba("extra_ambient_color"), Rgba("moonlight_color"),
			Float("extra_ambient_color_weight"), Float("extra_param"), Float("parameter_0"),
			Float("parameter_1"),
		], &[], &[Float("hue_shift")]),

		1 => (&[
			Rgba("color_0"), Rgba("color_1"), Rgba("color_2"), Float("elevation_0_degrees"),
			Float("elevation_1_degrees"), Float("elevation_2_degrees"), Float("rotation_degrees"),
		], &[], &[]),

		2 => (&[
			Unsigned("main_cloud"), Unsigned("alt_cloud"), Float("main_intensity"),
			Float("alt_intensity"), Rgba("diffuse_color"), Rgba("ambient_color"),
		], &[], &[]),

		3..=5 => (&[
			Float("density"), Float("weight"), Float("oscillation_spread"),
			Float("oscillation_frequency"), Float("distance_response_profile"),
			Float("extra_param"), Float("modulation_rate"), Rgba("color"), Unsigned("flags"),
		], &[], &[]),

		6 => (&[
			Float("layer_0_azimuth_degrees"), Float("unknown"), Float("layer_0_max_strength"),
		], &[], &[
			Float("layer_1_azimuth_degrees"), Float("layer_0_wavelength"),
			Float("layer_1_max_strength"), Float("layer_1_wavelength"),
			Float("layer_0_min_strength"), Float("layer_1_min_strength"),
		]),

		7 => (&[
			Unsigned("unknown"), Rgba("color_0"), Rgba("radiance_color"), Float("scale"),
			Float("some_param"),
		], &[], &[]),

		8 => (&[
			Float("unknown"), Float("world_wetness_parameter_1"),
			Float("world_wetness_parameter_0"), Float("character_wetness"),
		], &[], &[]),

		9 => (&[
			Float("adaptation_rate"), Float("adapted_luminance_parameter_w"),
			Float("adapted_luminance_parameter_x"), Float("adapted_luminance_parameter_y"),
			Float("tone_map_parameter_y"), Float("tone_map_parameter_x"),
		], &[], &[]),

		10 => (&[
			Float("hue"), Float("saturation"), Float("brightness"), Float("contrast"),
			Rgba("filter_color"), Float("filter_intensity"), Float("sepia"), Float("grayscale"),
			Float("negative"), Float("lut_input_black_point"), Float("lut_input_white_point"),
			Flag("alternate_curve_layout"), Padding(3),
		], &[], &[
			Float("dark_filter_saturation"), Float("dark_filter_parameter_x"),
			Float("dark_filter_parameter_y"), Float("dark_filter_tint_amount_and_parameter_z"),
			Rgba("dark_filter_tint_color"),
		]),

		11 => (&[
			Paths, Rgba("background_tint_color"), Rgba("foreground_tint_color"),
			Byte("foreground_effect_type"), Padding(3), Float("effect_transition_seconds"),
			Rgba("unknown_rgba_color"), Rgba("thunder_color"), Float("thunder_interval"),
			Float("background_intensity"), Float("foreground_intensity"),
		], &["effect_0", "effect_1"], &[]),

		12 => (&[
			Float("a_intensity"), Float("b_intensity"), Float("c_intensity"), Float("unknown"),
			Rgba("moon_color"), Float("unknown_2"), Float("procedural_star_intensity"),
		], &[], &[]),

		13 => (&[
			Rgba("fog_color"), Float("fog_start_distance"), Float("fog_intensity_0"),
			Float("fog_fade_distance"), Float("fog_intensity_1"), Float("fog_parameter"),
			Float("fog_blend"),
		], &[], &[
			Float("fog_density_percent"), Float("exp_fog_height"), Float("fog_height_falloff"),
			Float("start_distance"), Float("fog_min_opacity"), Float("fog_density_2_percent"),
			Float("exp_fog_height_2_delta"), Float("fog_height_falloff_2"),
			Float("directional_inscattering_start_distance"),
			Float("directional_inscattering_color_intensity"),
			Float("directional_inscattering_exponent"), Rgba("directional_inscattering_color"),
			Byte("use_height_fog_update"), Padding(3),
		]),

		// Named by neither the format nor anything that reads it, and written by no file the game
		// ships.
		14 => (&[
			Float("scalar_0"), Float("scalar_1"), Float("scalar_2"), Float("scalar_3"),
			Float("scalar_4"), Float("scalar_5"), Float("scalar_6"), Float("scalar_7"),
		], &[], &[]),

		20 => (&[Paths], &[
			"background_loop", "spot_ambience", "special_spot", "reserved_path",
			"random_grass_wind",
		], &[]),

		21 => (&[
			Flag("ambient_setting_0_enabled"), Flag("ambient_setting_1_enabled"), Padding(2),
		], &[], &[]),

		29 => (&[
			Short("transition_duration_centiseconds"), Flag("visible"), Padding(1),
		], &[], &[]),

		30 | 32 => (&[Float("value")], &[], &[]),

		31 => (&[Float("phase_rate"), Float("amplitude")], &[], &[]),

		33 | 35 => (&[Rgba("color")], &[], &[]),

		34 => (&[Rgba("color_0"), Rgba("color_1")], &[], &[]),

		_ => return None,
	};
	Some(Layout { fields, paths, extension })
}

/// The environment set a file opening with `magic` holds.
pub(super) fn read(mut stream: impl FileStream, magic: &[u8; 4]) -> Result<Environments> {
	let mut bytes = Vec::new();
	stream.read_to_end(&mut bytes)?;

	if bytes.get(..4) != Some(magic) || bytes.get(HEADER..HEADER + 4) != Some(b"ENVS") {
		return Err(invalid(format!(
			"not an {} file",
			String::from_utf8_lossy(magic)
		)));
	}

	// Offsets inside the section are measured from its body, which follows the magic and length.
	let body = HEADER + 8;
	let table = seek(&bytes, body, body + 4)?;
	let count = entries(&bytes, u32_at(&bytes, body + 8)?, table, WEATHER)?;

	let at = seek(&bytes, body, body + 12)?;
	let switches = bytes
		.get(at..at + OPTIONS.len())
		.ok_or_else(|| invalid(format!("switches at {at:#x} are past the end of the file")))?;

	Ok(Environments {
		version: u32_at(&bytes, body)?,
		options: std::array::from_fn(|index| switches[index] != 0),
		weathers: (0..count)
			.map(|index| Weather::parse(&bytes, table + index * WEATHER))
			.collect::<Result<_>>()?,
	})
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use super::{Value, layout, read};

	/// Builds a set of `weathers`, each holding one wetness set of one keyframe, with the section
	/// header the format states rather than one the reader assumes.
	fn build(magic: &[u8; 4], weathers: &[u32]) -> Vec<u8> {
		let mut bytes = Vec::from(*magic);
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(1u32.to_le_bytes());

		bytes.extend(*b"ENVS");
		bytes.extend(24u32.to_le_bytes());
		bytes.extend(6u32.to_le_bytes());
		bytes.extend(16u32.to_le_bytes());
		bytes.extend(u32::try_from(weathers.len()).unwrap().to_le_bytes());
		bytes.extend((16 + 16 * weathers.len() as u32 + 56).to_le_bytes());

		let rest = 16 * weathers.len();
		for (index, &weather) in weathers.iter().enumerate() {
			let footer = i32::try_from(rest - 16 * index).unwrap();
			bytes.extend((footer + 20).to_le_bytes());
			bytes.extend(1u32.to_le_bytes());
			bytes.extend(weather.to_le_bytes());
			bytes.extend(footer.to_le_bytes());
		}

		bytes.extend(86400f32.to_le_bytes());
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(1f32.to_le_bytes());
		bytes.extend(4u32.to_le_bytes());
		bytes.extend(4u32.to_le_bytes());

		bytes.extend(12u32.to_le_bytes());
		bytes.extend(1u32.to_le_bytes());
		bytes.extend(8u32.to_le_bytes());

		bytes.extend(4u32.to_le_bytes());
		bytes.extend(3600f32.to_le_bytes());
		bytes.extend((0..4).flat_map(|index| (index as f32).to_le_bytes()));

		bytes.extend([1u8, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
		bytes
	}

	#[test]
	fn reads_the_weathers_a_set_covers() {
		let file = read(Cursor::new(build(b"ENVB", &[1, 2, 59])), b"ENVB").unwrap();
		assert_eq!(file.version(), 6);
		assert_eq!(file.options()[..4], [true, false, false, true]);

		let ids: Vec<u32> = file.weathers().iter().map(|weather| weather.id()).collect();
		assert_eq!(ids, [1, 2, 59]);
	}

	#[test]
	fn reads_the_keyframes_of_every_set() {
		let file = read(Cursor::new(build(b"ENVB", &[1, 2])), b"ENVB").unwrap();

		let weather = &file.weathers()[1];
		assert_eq!(weather.length(), 86400.);
		assert_eq!(weather.paths(), &[String::new(), String::new()]);

		let sets = weather.sets();
		assert_eq!(sets.len(), 1);
		assert_eq!(sets[0].kind(), 8);
		assert_eq!(sets[0].name(), Some("Wetness"));

		let keyframes = sets[0].keyframes();
		assert_eq!(keyframes.len(), 1);
		assert_eq!(keyframes[0].time(), 3600.);
		assert_eq!(keyframes[0].colours().count(), 0);
		assert_eq!(
			keyframes[0].fields(),
			[
				("unknown", Value::Float(0.)),
				("world_wetness_parameter_1", Value::Float(1.)),
				("world_wetness_parameter_0", Value::Float(2.)),
				("character_wetness", Value::Float(3.)),
			]
		);
	}

	/// Every structure is reached by an offset, so nothing may be read on the word of a header
	/// alone.
	#[test]
	fn truncated() {
		let mut bytes = build(b"ENVB", &[1, 2]);
		bytes.truncate(bytes.len() - 1);
		assert!(read(Cursor::new(bytes), b"ENVB").is_err());
	}

	#[test]
	fn rejects_another_format() {
		assert!(read(Cursor::new(build(b"ESSB", &[1])), b"ENVB").is_err());
	}

	/// The fields of a kind have to account for every byte of it, since the fields it grew are
	/// looked for at the end of the ones it started with.
	#[test]
	fn every_kind_covers_the_bytes_it_takes() {
		let sizes = [
			(0, 44, 8),
			(1, 32, 0),
			(2, 28, 0),
			(3, 40, 0),
			(4, 40, 0),
			(5, 40, 0),
			(6, 16, 28),
			(7, 24, 0),
			(8, 20, 0),
			(9, 28, 0),
			(10, 52, 24),
			(11, 48, 0),
			(12, 32, 0),
			(13, 32, 56),
			(14, 36, 0),
			(20, 12, 0),
			(21, 8, 0),
			(29, 8, 0),
			(30, 8, 0),
			(31, 12, 0),
			(32, 8, 0),
			(33, 8, 0),
			(34, 12, 0),
			(35, 8, 0),
		];
		for (kind, size, extension) in sizes {
			let it = layout(kind).unwrap_or_else(|| panic!("no layout for kind {kind}"));
			assert_eq!(it.size(), size, "size of kind {kind}");
			let grown = match it.extension.is_empty() {
				true => 0,
				false => 4 + it.extension.iter().map(super::Field::size).sum::<usize>(),
			};
			assert_eq!(grown, extension, "extension of kind {kind}");
		}
	}

	/// Where in a keyframe each of its colour offsets sits. A colour is the one field reached
	/// through the file rather than read in place, so an offset at the wrong position reads
	/// whatever the neighbouring field happens to hold.
	#[test]
	fn every_colour_offset_sits_where_it_does_in_the_file() {
		let colours = |fields: &[super::Field], from: usize| {
			let mut at = from;
			let mut found = Vec::new();
			for field in fields {
				if matches!(field, super::Field::Colour(_)) {
					found.push(at);
				}
				at += field.size();
			}
			found
		};

		for (kind, base, extension) in [
			(0, &[4, 20, 24][..], &[][..]),
			(1, &[4, 8, 12], &[]),
			(2, &[20, 24], &[]),
			(3, &[32], &[]),
			(4, &[32], &[]),
			(5, &[32], &[]),
			(6, &[], &[]),
			(7, &[8, 12], &[]),
			(10, &[20], &[20]),
			(11, &[12, 16, 28, 32], &[]),
			(12, &[20], &[]),
			(13, &[4], &[48]),
			(33, &[4], &[]),
			(34, &[4, 8], &[]),
			(35, &[4], &[]),
		] {
			let it = layout(kind).unwrap();
			assert_eq!(colours(it.fields, 4), base, "colours of kind {kind}");
			// An offset the fields it grew carry is still measured from the keyframe's own start.
			let grown = colours(it.extension, it.size() + 4)
				.iter()
				.map(|at| at - it.size())
				.collect::<Vec<_>>();
			assert_eq!(grown, extension, "grown colours of kind {kind}");
		}
	}

	#[test]
	fn a_kind_the_format_does_not_name() {
		assert!(layout(15).is_none());
		assert!(layout(36).is_none());
	}
}
