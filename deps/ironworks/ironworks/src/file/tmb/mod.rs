//! Structs and utilities for parsing .tmb files.

pub mod command;

pub use command::{Command, CommandKind};

use std::io::{Read, Seek, SeekFrom};

use binrw::{BinRead, BinResult, Endian, VecArgs, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// A timeline: what a character does over the course of one animation, as commands grouped into
/// tracks and tracks into actors.
///
/// The same format is embedded in `.pap` and `.cutb`, so a timeline is read from wherever the
/// stream is positioned rather than only from the start of a file.
#[binread]
#[br(little, magic = b"TMLB")]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Timeline {
	// The skipped size covers the whole timeline including this header.
	#[br(temp, pad_before = 4)]
	item_count: u32,

	#[br(temp, restore_position, if(item_count > 0, *b"TMDH"))]
	sniff: [u8; 4],

	/// Which of the two item layouts the timeline uses.
	#[br(calc = Layout::of(&sniff))]
	layout: Layout,

	#[br(parse_with = items, args(layout, item_count))]
	#[getset(skip)]
	items: Vec<Item>,
}

impl Timeline {
	/// Every item, in the order the file lists them.
	pub fn items(&self) -> &[Item] {
		&self.items
	}
}

impl File for Timeline {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// The two item layouts the `TMLB` magic is used for.
///
/// Told apart by content: a standard timeline's item array starts at 0x0C, so a magic sits there,
/// where a wide one repeats its item count instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
	/// Items lead with a 4-byte magic, a `u32` size and, bar `TMPP` and `TMAL`, an `i16` id;
	/// variable data is reached from `item_start + 8`.
	Standard,

	/// Items spend eight further bytes before a 32-bit id and reach variable data from
	/// `item_start + 0x10`, and command bodies are wider. Only the header and the item extents are
	/// modelled, so every item reads as [`Item::Unknown`].
	Wide,
}

impl Layout {
	fn of(sniff: &[u8; 4]) -> Self {
		match sniff
			.iter()
			.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
		{
			true => Self::Standard,
			false => Self::Wide,
		}
	}

	/// Bytes an item spends before its body.
	fn preamble(self) -> u64 {
		match self {
			Self::Standard => 8,
			Self::Wide => 16,
		}
	}

	/// Bytes of header past the item count, which the item array follows.
	fn padding(self) -> u64 {
		match self {
			Self::Standard => 0,
			Self::Wide => 20,
		}
	}
}

/// One item of a timeline.
#[derive(Debug)]
pub enum Item {
	/// `TMDH`, the timeline's own parameters.
	Header(Header),

	/// `TMPP`, the facial expression library the timeline plays against.
	FaceLibrary(FaceLibrary),

	/// `TMAL`, the actors the timeline drives.
	ActorList(ActorList),

	/// `TMAC`, one actor.
	Actor(Actor),

	/// `TMTR`, one track of commands.
	Track(Track),

	/// `TMFC`, a set of f-curves.
	Curves(Curves),

	/// A `Cxxx` command.
	Command(Command),

	/// An item whose magic this crate does not model.
	Unknown(Unknown),
}

impl Item {
	/// The id other items reference this one by, for the kinds that carry one.
	pub fn id(&self) -> Option<i16> {
		match self {
			Self::Header(header) => Some(header.id),
			Self::Actor(actor) => Some(actor.id),
			Self::Track(track) => Some(track.id),
			Self::Curves(curves) => Some(curves.id),
			Self::Command(command) => Some(command.id()),
			Self::FaceLibrary(_) | Self::ActorList(_) | Self::Unknown(_) => None,
		}
	}
}

/// `TMDH`: the timeline's own parameters.
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Header {
	id: i16,

	/// Zero in every file the game ships.
	unknown_1: i16,

	/// How long the timeline runs, in the same units as a command's time.
	duration: i16,

	/// Three in every file the game ships.
	unknown_3: i16,
}

/// `TMPP`: the facial expression library the timeline plays against.
///
/// The item is only written when a library is used, so its absence is the absence of the item.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug)]
pub struct FaceLibrary {
	#[br(parse_with = offset_string, args(base))]
	path: Option<String>,
}

impl FaceLibrary {
	/// Path of the library, as a `.pap`.
	pub fn path(&self) -> Option<&str> {
		self.path.as_deref()
	}
}

/// `TMAL`: the actors the timeline drives.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug)]
pub struct ActorList {
	#[br(parse_with = offset_ids, args(base))]
	actors: Vec<i16>,
}

impl ActorList {
	/// Ids of the `TMAC` items this timeline plays.
	pub fn actors(&self) -> &[i16] {
		&self.actors
	}
}

/// `TMAC`: one actor, holding the tracks played against a single participant.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Actor {
	id: i16,

	time: i16,

	ability_delay: i32,

	unknown_2: i32,

	#[br(parse_with = offset_ids, args(base))]
	#[getset(skip)]
	tracks: Vec<i16>,
}

impl Actor {
	/// Ids of the `TMTR` items this actor plays.
	pub fn tracks(&self) -> &[i16] {
		&self.tracks
	}
}

/// `TMTR`: one track, holding commands that run together, optionally behind a condition.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Track {
	id: i16,

	time: i16,

	#[br(parse_with = offset_ids, args(base))]
	#[getset(skip)]
	commands: Vec<i16>,

	#[br(parse_with = offset_condition, args(base))]
	#[getset(skip)]
	condition: Vec<Condition>,
}

impl Track {
	/// Ids of the `Cxxx` items this track runs.
	pub fn commands(&self) -> &[i16] {
		&self.commands
	}

	/// The condition gating the track, in postfix order. Empty where the track carries none.
	pub fn condition(&self) -> &[Condition] {
		&self.condition
	}
}

/// One step of a track's condition.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Condition {
	/// Which operation to apply, over the `0x00..=0x31` range VFXEditor's `LuaOperation` names.
	operation: u32,

	/// Operand of an integer, parenthesis-count or variable operation. A variable holds its pool in
	/// the top four bits and its index in the rest.
	value: u32,

	/// Operand of a float operation.
	float: f32,
}

/// `TMFC`: a set of f-curves, named by id from the movement and camera commands they drive.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Curves {
	id: i16,

	time: i16,

	#[br(temp)]
	curve_offset: i32,

	#[br(temp)]
	curve_count: u32,

	unknown_a: i32,

	/// Where the curve region ends, relative to the same place the curves themselves are reached
	/// from. Equal to the curve offset in the files whose keyframes precede their curves.
	end: i32,

	unknown_b: i32,

	#[br(parse_with = offset_curves, args(base, curve_offset, curve_count))]
	#[getset(skip)]
	curves: Vec<[u8; 16]>,
}

impl Curves {
	/// The curves, uninterpreted.
	///
	/// Each is 16 bytes and names its own keyframes, but the two encodings that ship disagree on
	/// how, so the keyframes themselves are not reached.
	pub fn curves(&self) -> &[[u8; 16]] {
		&self.curves
	}
}

/// An item whose magic this crate does not model.
#[derive(Debug, CopyGetters)]
#[get_copy = "pub"]
pub struct Unknown {
	magic: [u8; 4],

	#[getset(skip)]
	body: Vec<u8>,
}

impl Unknown {
	/// Everything the item holds past its magic and size.
	pub fn body(&self) -> &[u8] {
		&self.body
	}
}

/// Reads the item array, resyncing to each item's declared end.
fn items<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(layout, count): (Layout, u32),
) -> BinResult<Vec<Item>> {
	let length = length(reader)?;
	let mut at = reader.stream_position()? + layout.padding();

	let mut items = Vec::new();
	for _ in 0..count {
		reader.seek(SeekFrom::Start(at))?;
		let magic = <[u8; 4]>::read_options(reader, endian, ())?;
		let size = u64::from(u32::read_options(reader, endian, ())?);
		let end = at + size;
		if size < layout.preamble() || end > length {
			return Err(invalid(
				at,
				format!(
					"item {} declares {size} bytes at {at} of a {length} byte timeline",
					String::from_utf8_lossy(&magic)
				),
			));
		}

		items.push(item(reader, endian, layout, &magic, at, end)?);

		// Resync on the declared size, so one drifted item cannot desync the rest.
		at = end;
	}
	Ok(items)
}

fn item<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	layout: Layout,
	magic: &[u8; 4],
	start: u64,
	end: u64,
) -> BinResult<Item> {
	if layout == Layout::Wide {
		return unknown(reader, magic, end).map(Item::Unknown);
	}

	let base = start + 8;
	Ok(match magic {
		b"TMDH" => Item::Header(Header::read_options(reader, endian, ())?),
		b"TMPP" => Item::FaceLibrary(FaceLibrary::read_options(reader, endian, (base,))?),
		b"TMAL" => Item::ActorList(ActorList::read_options(reader, endian, (base,))?),
		b"TMAC" => Item::Actor(Actor::read_options(reader, endian, (base,))?),
		b"TMTR" => Item::Track(Track::read_options(reader, endian, (base,))?),
		b"TMFC" => Item::Curves(Curves::read_options(reader, endian, (base,))?),
		[b'C', digits @ ..] if digits.iter().all(u8::is_ascii_digit) => {
			Item::Command(Command::parse(reader, endian, magic, base, end)?)
		}
		_ => Item::Unknown(unknown(reader, magic, end)?),
	})
}

fn unknown<R: Read + Seek>(reader: &mut R, magic: &[u8; 4], end: u64) -> BinResult<Unknown> {
	Ok(Unknown {
		magic: *magic,
		body: rest(reader, end)?,
	})
}

/// The bytes from the cursor to `end`, which it must not already be past.
fn rest<R: Read + Seek>(reader: &mut R, end: u64) -> BinResult<Vec<u8>> {
	let at = reader.stream_position()?;
	let length = end.checked_sub(at).ok_or_else(|| {
		invalid(
			at,
			format!("item ends at {end}, before its own body at {at}"),
		)
	})?;
	let mut body = vec![0; usize::try_from(length).unwrap()];
	reader.read_exact(&mut body)?;
	Ok(body)
}

fn length<R: Seek>(reader: &mut R) -> BinResult<u64> {
	let resume = reader.stream_position()?;
	let length = reader.seek(SeekFrom::End(0))?;
	reader.seek(SeekFrom::Start(resume))?;
	Ok(length)
}

/// Position of `base + offset`, which `extent` further bytes of stream must follow. Leaves the
/// cursor where it found it.
///
/// The bound is inclusive of the stream's end, because an empty list legitimately sits there. It is
/// the stream's end rather than the timeline's, so a timeline read from the middle of a longer
/// stream is bounded more loosely than a standalone one.
fn resolve<R: Seek>(reader: &mut R, base: u64, offset: i32, extent: u64) -> BinResult<u64> {
	let length = length(reader)?;
	base.checked_add_signed(offset.into())
		.filter(|start| start.checked_add(extent).is_some_and(|end| end <= length))
		.ok_or_else(|| {
			invalid(
				base,
				format!(
					"{extent} bytes at offset {offset} from {base} lie outside the {length} byte timeline"
				),
			)
		})
}

/// Reads an `i32` offset relative to `base`, then `read` at what it names. `0` means absent.
fn at_offset<R: Read + Seek, T>(
	reader: &mut R,
	endian: Endian,
	base: u64,
	extent: u64,
	read: impl FnOnce(&mut R) -> BinResult<T>,
) -> BinResult<Option<T>> {
	let offset = i32::read_options(reader, endian, ())?;
	if offset == 0 {
		return Ok(None);
	}
	let start = resolve(reader, base, offset, extent)?;
	let resume = reader.stream_position()?;
	reader.seek(SeekFrom::Start(start))?;
	let value = read(reader)?;
	reader.seek(SeekFrom::Start(resume))?;
	Ok(Some(value))
}

/// Reads an `i32` offset relative to `base` and the `u32` element count beside it, then that many
/// `stride`-byte elements at what they name.
fn at_list<R: Read + Seek, T: for<'a> BinRead<Args<'a> = ()> + 'static>(
	reader: &mut R,
	endian: Endian,
	base: u64,
	stride: u64,
) -> BinResult<Vec<T>> {
	let offset = i32::read_options(reader, endian, ())?;
	let count = u32::read_options(reader, endian, ())?;
	if offset == 0 {
		return Ok(Vec::new());
	}
	let start = resolve(reader, base, offset, u64::from(count) * stride)?;
	let resume = reader.stream_position()?;
	reader.seek(SeekFrom::Start(start))?;
	let values = Vec::read_options(
		reader,
		endian,
		VecArgs {
			count: count as usize,
			inner: (),
		},
	)?;
	reader.seek(SeekFrom::Start(resume))?;
	Ok(values)
}

/// A NUL-terminated string at an `i32` offset relative to `base`.
fn offset_string<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(base,): (u64,),
) -> BinResult<Option<String>> {
	at_offset(reader, endian, base, 0, |reader| {
		let mut bytes = Vec::new();
		loop {
			match u8::read_options(reader, endian, ())? {
				0 => break,
				byte => bytes.push(byte),
			}
		}
		Ok(String::from_utf8_lossy(&bytes).into_owned())
	})
}

/// Floats at an `i32` offset and count relative to `base`. Three or four of them in every file the
/// game ships, matching the vector the field holds.
fn offset_floats<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(base,): (u64,),
) -> BinResult<Vec<f32>> {
	at_list(reader, endian, base, 4)
}

fn offset_ids<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(base,): (u64,),
) -> BinResult<Vec<i16>> {
	at_list(reader, endian, base, 2)
}

/// Reads a track's condition, whose own header holds the step count.
fn offset_condition<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(base,): (u64,),
) -> BinResult<Vec<Condition>> {
	let steps = at_offset(reader, endian, base, 8, |reader| {
		// The leading field is 8 in every track that carries a condition, and is the stride of the
		// header it sits in rather than anything variable.
		let _ = u32::read_options(reader, endian, ())?;
		let count = u32::read_options(reader, endian, ())?;
		let start = reader.stream_position()?;
		resolve(reader, start, 0, u64::from(count) * 12)?;
		Vec::read_options(
			reader,
			endian,
			VecArgs {
				count: count as usize,
				inner: (),
			},
		)
	})?;
	Ok(steps.unwrap_or_default())
}

/// Reads the 16-byte curve records a `TMFC` names.
fn offset_curves<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(base, offset, count): (u64, i32, u32),
) -> BinResult<Vec<[u8; 16]>> {
	if offset == 0 {
		return Ok(Vec::new());
	}
	// A curve region is reached from `item_start + 12`, four bytes past the base every other item
	// uses; VFXEditor gets to the same place by adding four to the offset instead.
	let start = resolve(reader, base + 4, offset, u64::from(count) * 16)?;
	let resume = reader.stream_position()?;
	reader.seek(SeekFrom::Start(start))?;
	let records = Vec::read_options(
		reader,
		endian,
		VecArgs {
			count: count as usize,
			inner: (),
		},
	)?;
	reader.seek(SeekFrom::Start(resume))?;
	Ok(records)
}

fn invalid(pos: u64, message: String) -> binrw::Error {
	binrw::Error::AssertFail { pos, message }
}

#[cfg(test)]
mod test;
