//! Structs and utilities for parsing .avfx files.
//!
//! An effect is a tree of tagged, length-prefixed blocks. Every block is a four-character tag
//! written back to front, a length, and a payload padded out to four bytes; a payload is either
//! more blocks or a value. Nothing in the file says which, so ironworks nests where a payload
//! divides exactly into well-formed blocks and keeps the bytes otherwise.
//!
//! The root holds the effect's own settings, then eight lists: [schedulers](Avfx::schedulers),
//! [timelines](Avfx::timelines), [emitters](Avfx::emitters), [particles](Avfx::particles),
//! [effectors](Avfx::effectors), [binders](Avfx::binders), [textures](Avfx::textures) and
//! [models](Avfx::models). Each list is preceded by its length and the lists always appear in that
//! order. A scheduler starts timelines, a timeline runs emitters over a span of frames, and an
//! emitter spawns particles and further emitters; the references between them are indices into
//! these lists, written `-1` where absent.
//!
//! Anything animated is a curve: a block carrying a `Keys` list of [keyframes](CurveKey), with
//! `BvPr` and `BvPo` beside it saying what happens either side of the keyed span. Curves over more
//! than one axis nest one curve per axis under `X`, `Y`, `Z` and their `R`-suffixed random
//! counterparts.
//!
//! Only the root's settings are named here. The tags below it number in the hundreds and are named
//! nowhere but in VFXEditor, whose readings the other public readers repeat rather than corroborate,
//! so ironworks leaves them addressable by tag.

mod block;
mod curve;
mod model;
mod node;

pub use {
	block::{Block, Name, Payload},
	curve::{CurveBehaviour, CurveKey, KeyKind, RandomKind},
	model::{EmitVertex, Model, Triangle, Vertex},
	node::{Clip, ClipKind, Emitter, Item, Scheduler, Timeline},
};

use getset::CopyGetters;

use crate::{
	FileStream,
	error::{Error, ErrorValue, Result},
};

use super::File;
use block::find;

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("AVFX".into()), reason.into())
}

/// An animated visual effect.
#[derive(Debug, CopyGetters)]
pub struct Avfx {
	/// `0x20110913` in every file the game ships.
	#[get_copy = "pub"]
	version: u32,

	properties: Vec<Block>,

	schedulers: Vec<Scheduler>,
	timelines: Vec<Timeline>,
	emitters: Vec<Emitter>,
	particles: Vec<Block>,
	effectors: Vec<Block>,
	binders: Vec<Block>,
	textures: Vec<String>,
	models: Vec<Model>,
}

impl Avfx {
	/// The blocks holding the effect's own settings, in file order. The named accessors below read
	/// from these; anything they do not cover is still here under its tag.
	pub fn properties(&self) -> &[Block] {
		&self.properties
	}

	/// What starts the effect's timelines.
	pub fn schedulers(&self) -> &[Scheduler] {
		&self.schedulers
	}

	/// The spans of frames the effect runs over.
	pub fn timelines(&self) -> &[Timeline] {
		&self.timelines
	}

	/// What spawns the effect's particles.
	pub fn emitters(&self) -> &[Emitter] {
		&self.emitters
	}

	/// What the effect draws, one `Ptcl` block each. `Data` holds whatever the particle's `PrVT`
	/// kind adds, and the curves driving it sit beside it.
	pub fn particles(&self) -> &[Block] {
		&self.particles
	}

	/// The lights and screen effects a timeline can run, one `Efct` block each.
	pub fn effectors(&self) -> &[Block] {
		&self.effectors
	}

	/// What binds an effect to something in the world, one `Bind` block each.
	pub fn binders(&self) -> &[Block] {
		&self.binders
	}

	/// The textures the effect samples, as paths to `.atex` files.
	pub fn textures(&self) -> &[String] {
		&self.textures
	}

	/// The geometry the effect draws or emits from.
	pub fn models(&self) -> &[Model] {
		&self.models
	}

	/// `bDFP`.
	pub fn is_delay_fast_particle(&self) -> Option<bool> {
		self.flag("bDFP")
	}

	/// `bFG`.
	pub fn is_fit_ground(&self) -> Option<bool> {
		self.flag("bFG")
	}

	/// `bTS`.
	pub fn is_transform_skip(&self) -> Option<bool> {
		self.flag("bTS")
	}

	/// `bASH`.
	pub fn is_all_stop_on_hide(&self) -> Option<bool> {
		self.flag("bASH")
	}

	/// `bCBC`.
	pub fn can_be_clipped_out(&self) -> Option<bool> {
		self.flag("bCBC")
	}

	/// `bCmS`.
	pub fn is_camera_space(&self) -> Option<bool> {
		self.flag("bCmS")
	}

	/// `bFEL`.
	pub fn is_full_env_light(&self) -> Option<bool> {
		self.flag("bFEL")
	}

	/// `bCul`, gating [`clip_box`](Self::clip_box).
	pub fn clip_box_enabled(&self) -> Option<bool> {
		self.flag("bCul")
	}

	/// `CBPx`, `CBPy` and `CBPz`: where the box the effect is culled against sits.
	pub fn clip_box(&self) -> Option<[f32; 3]> {
		self.vector(["CBPx", "CBPy", "CBPz"])
	}

	/// `CBSx`, `CBSy` and `CBSz`: how large that box is.
	pub fn clip_box_size(&self) -> Option<[f32; 3]> {
		self.vector(["CBSx", "CBSy", "CBSz"])
	}

	/// `ZBMs`.
	pub fn bias_z_max_scale(&self) -> Option<f32> {
		self.float("ZBMs")
	}

	/// `ZBMd`.
	pub fn bias_z_max_distance(&self) -> Option<f32> {
		self.float("ZBMd")
	}

	/// `bOSt`. Where it is set, the effect carries its own [near](Self::near_clip) and
	/// [far](Self::far_clip) clip distances rather than taking the ones around it.
	pub fn clip_own_setting(&self) -> Option<bool> {
		self.flag("bOSt")
	}

	/// `NCB` and `NCE`: where the effect starts and finishes fading in as it nears the camera.
	pub fn near_clip(&self) -> Option<(f32, f32)> {
		Some((self.float("NCB")?, self.float("NCE")?))
	}

	/// `FCB` and `FCE`: where it starts and finishes fading out as it recedes.
	pub fn far_clip(&self) -> Option<(f32, f32)> {
		Some((self.float("FCB")?, self.float("FCE")?))
	}

	/// `SPFR`.
	pub fn soft_particle_fade_range(&self) -> Option<f32> {
		self.float("SPFR")
	}

	/// `SKO`. VFXEditor names the field for a soft key and labels it for a sort key.
	pub fn sort_key_offset(&self) -> Option<f32> {
		self.float("SKO")
	}

	/// `DwLy`.
	pub fn draw_layer(&self) -> Option<DrawLayer> {
		Some(self.integer("DwLy")?.into())
	}

	/// `DwOT`.
	pub fn draw_order(&self) -> Option<DrawOrder> {
		Some(self.integer("DwOT")?.into())
	}

	/// `DLST`.
	pub fn directional_light_source(&self) -> Option<DirectionalLightSource> {
		Some(self.integer("DLST")?.into())
	}

	/// `PL1S` and `PL2S`, the two point lights the effect can be lit by.
	pub fn point_light_sources(&self) -> [Option<PointLightSource>; 2] {
		["PL1S", "PL2S"].map(|name| Some(self.integer(name)?.into()))
	}

	/// `RvPx`, `RvPy` and `RvPz`: a translation applied over whatever placed the effect.
	pub fn revised_position(&self) -> Option<[f32; 3]> {
		self.vector(["RvPx", "RvPy", "RvPz"])
	}

	/// `RvRx`, `RvRy` and `RvRz`: a rotation applied the same way, in radians.
	pub fn revised_rotation(&self) -> Option<[f32; 3]> {
		self.vector(["RvRx", "RvRy", "RvRz"])
	}

	/// `RvSx`, `RvSy` and `RvSz`: a scale applied the same way.
	pub fn revised_scale(&self) -> Option<[f32; 3]> {
		self.vector(["RvSx", "RvSy", "RvSz"])
	}

	/// `RvR`, `RvG` and `RvB`: a colour the effect is multiplied by.
	pub fn revised_colour(&self) -> Option<[f32; 3]> {
		self.vector(["RvR", "RvG", "RvB"])
	}

	/// `AFXe`, `AFXi` and `AFXo`: how the effect fades out along the X axis.
	pub fn fade_x(&self) -> Option<Fade> {
		self.fade(["AFXe", "AFXi", "AFXo"])
	}

	/// `AFYe`, `AFYi` and `AFYo`: the same along Y.
	pub fn fade_y(&self) -> Option<Fade> {
		self.fade(["AFYe", "AFYi", "AFYo"])
	}

	/// `AFZe`, `AFZi` and `AFZo`: the same along Z.
	pub fn fade_z(&self) -> Option<Fade> {
		self.fade(["AFZe", "AFZi", "AFZo"])
	}

	/// `bGFE`, gating [`global_fog_influence`](Self::global_fog_influence).
	pub fn global_fog_enabled(&self) -> Option<bool> {
		self.flag("bGFE")
	}

	/// `GFIM`.
	pub fn global_fog_influence(&self) -> Option<f32> {
		self.float("GFIM")
	}

	fn flag(&self, name: &str) -> Option<bool> {
		find(&self.properties, name)?.bool()
	}

	fn float(&self, name: &str) -> Option<f32> {
		find(&self.properties, name)?.f32()
	}

	fn integer(&self, name: &str) -> Option<i32> {
		find(&self.properties, name)?.i32()
	}

	fn vector(&self, names: [&str; 3]) -> Option<[f32; 3]> {
		match names.map(|name| self.float(name)) {
			[Some(x), Some(y), Some(z)] => Some([x, y, z]),
			_ => None,
		}
	}

	fn fade(&self, names: [&str; 3]) -> Option<Fade> {
		Some(Fade {
			enabled: self.flag(names[0])?,
			inner: self.float(names[1])?,
			outer: self.float(names[2])?,
		})
	}

	fn parse(blocks: Vec<Block>) -> Result<Self> {
		let version = find(&blocks, "Ver")
			.and_then(Block::i32)
			.ok_or_else(|| invalid("no version"))? as u32;

		let mut file = Self {
			version,
			properties: Vec::new(),
			schedulers: Vec::new(),
			timelines: Vec::new(),
			emitters: Vec::new(),
			particles: Vec::new(),
			effectors: Vec::new(),
			binders: Vec::new(),
			textures: Vec::new(),
			models: Vec::new(),
		};

		for block in blocks {
			match block.name().as_str() {
				"Ver" => (),
				"ScCn" | "TlCn" | "EmCn" | "PrCn" | "EfCn" | "BdCn" | "TxCn" | "MdCn" => (),
				"Schd" => file.schedulers.push(Scheduler::parse(block.into_blocks())),
				"TmLn" => file.timelines.push(Timeline::parse(block.into_blocks())),
				"Emit" => file.emitters.push(Emitter::parse(block.into_blocks())),
				"Ptcl" => file.particles.push(block),
				"Efct" => file.effectors.push(block),
				"Bind" => file.binders.push(block),
				"Tex" => file.textures.push(block.text().unwrap_or_default()),
				"Modl" => file.models.push(Model::parse(block.blocks())?),
				_ => file.properties.push(block),
			}
		}

		Ok(file)
	}
}

impl File for Avfx {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		let mut bytes = Vec::new();
		stream.read_to_end(&mut bytes)?;

		let mut blocks = Block::parse(&bytes)?;
		let root = match blocks.len() {
			1 => blocks.pop().unwrap(),
			count => {
				return Err(invalid(format!(
					"file holds {count} blocks rather than one"
				)));
			}
		};
		if root.name() != "AVFX" {
			return Err(invalid(format!("file opens with {} block", root.name())));
		}

		Self::parse(root.into_blocks())
	}
}

/// How an effect fades out towards the edge of one axis.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Fade {
	enabled: bool,

	/// Where the fade begins.
	inner: f32,

	/// Where it finishes.
	outer: f32,
}

/// Which pass an effect is drawn in.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawLayer {
	Screen,
	BaseUpper,
	Base,
	BaseLower,
	InWater,
	BeforeCloud,
	BehindCloud,
	BeforeSky,
	PostUi,
	PrevUi,
	FitWater,
	/// A layer ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for DrawLayer {
	fn from(value: i32) -> Self {
		match value {
			0 => Self::Screen,
			1 => Self::BaseUpper,
			2 => Self::Base,
			3 => Self::BaseLower,
			4 => Self::InWater,
			5 => Self::BeforeCloud,
			6 => Self::BehindCloud,
			7 => Self::BeforeSky,
			8 => Self::PostUi,
			9 => Self::PrevUi,
			10 => Self::FitWater,
			other => Self::Unknown(other),
		}
	}
}

/// How an effect's particles are sorted against each other.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOrder {
	Default,
	Reverse,
	Depth,
	/// An order ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for DrawOrder {
	fn from(value: i32) -> Self {
		match value {
			0 => Self::Default,
			1 => Self::Reverse,
			2 => Self::Depth,
			other => Self::Unknown(other),
		}
	}
}

/// Where an effect takes its directional light from.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectionalLightSource {
	None,
	InLocal,
	InGame,
	/// A source ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for DirectionalLightSource {
	fn from(value: i32) -> Self {
		match value {
			0 => Self::None,
			1 => Self::InLocal,
			2 => Self::InGame,
			other => Self::Unknown(other),
		}
	}
}

/// Where an effect takes one of its two point lights from.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointLightSource {
	None,
	CreateTimeBackground,
	AlwaysBackground,
	LocalVfx,
	GlobalVfx,
	/// A source ironworks does not recognise; the inner value is the raw tag.
	Unknown(i32),
}

impl From<i32> for PointLightSource {
	fn from(value: i32) -> Self {
		match value {
			0 => Self::None,
			1 => Self::CreateTimeBackground,
			2 => Self::AlwaysBackground,
			3 => Self::LocalVfx,
			4 => Self::GlobalVfx,
			other => Self::Unknown(other),
		}
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::{Avfx, ClipKind, KeyKind, Payload};

	/// One block: the tag back to front, then its length, then its payload padded out to four.
	fn block(tag: &str, payload: &[u8]) -> Vec<u8> {
		let mut bytes = tag.bytes().rev().collect::<Vec<_>>();
		bytes.resize(4, 0);
		bytes.extend(u32::try_from(payload.len()).unwrap().to_le_bytes());
		bytes.extend(payload);
		bytes.resize(8 + payload.len().next_multiple_of(4), 0);
		bytes
	}

	fn nest(tag: &str, children: &[Vec<u8>]) -> Vec<u8> {
		block(tag, &children.concat())
	}

	fn read(children: &[Vec<u8>]) -> Avfx {
		let mut blocks = vec![block("Ver", &0x2011_0913u32.to_le_bytes())];
		blocks.extend_from_slice(children);
		Avfx::read(Cursor::new(nest("AVFX", &blocks))).unwrap()
	}

	fn integer(value: i32) -> Vec<u8> {
		value.to_le_bytes().into()
	}

	/// One scheduler or timeline entry, which starts at the first tag it carries.
	fn item(timeline: i32) -> Vec<u8> {
		[
			block("bEna", &integer(1)),
			block("StTm", &integer(0)),
			block("TlNo", &integer(timeline)),
		]
		.concat()
	}

	#[test]
	fn empty() {
		assert!(matches!(Avfx::read(io::empty()), Err(Error::Invalid(..))));
	}

	#[test]
	fn wrong_magic() {
		let bytes = nest("MDL", &[block("Ver", &integer(1))]);
		assert!(matches!(
			Avfx::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn block_declaring_more_than_it_holds() {
		let mut bytes = nest("AVFX", &[block("Ver", &integer(1))]);
		bytes.truncate(bytes.len() - 4);
		assert!(matches!(
			Avfx::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	/// Whether a payload nests is decided by reading a size out of it, so any byte pattern can name
	/// one that no pointer could hold.
	#[test]
	fn block_declaring_more_than_a_pointer_holds() {
		let mut bytes = nest("AVFX", &[block("Ver", &integer(1))]);
		bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
		assert!(matches!(
			Avfx::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn tags_are_written_back_to_front() {
		let file = read(&[
			block("bFG", &integer(1)),
			block("SKO", &1.5f32.to_le_bytes()),
		]);
		assert_eq!(file.version(), 0x2011_0913);
		assert_eq!(file.properties().len(), 2);
		assert_eq!(file.properties()[0].name(), "bFG");
		assert_eq!(file.is_fit_ground(), Some(true));
		assert_eq!(file.sort_key_offset(), Some(1.5));
	}

	#[test]
	fn flags_are_written_one_or_four_bytes_wide() {
		let file = read(&[block("bTS", &[1]), block("bASH", &integer(0))]);
		assert_eq!(file.is_transform_skip(), Some(true));
		assert_eq!(file.is_all_stop_on_hide(), Some(false));
	}

	#[test]
	fn absent_properties_read_as_none() {
		let file = read(&[block("CBPx", &1.0f32.to_le_bytes())]);
		assert_eq!(file.clip_box(), None);
		assert_eq!(file.near_clip(), None);
		assert_eq!(file.draw_layer(), None);
	}

	#[test]
	fn a_payload_of_blocks_nests() {
		let file = read(&[nest("Ptcl", &[nest("Pos", &[block("ACT", &integer(2))])])]);

		let position = file.particles()[0].find("Pos").unwrap();
		assert!(matches!(position.payload(), Payload::Blocks(_)));
		assert_eq!(position.find("ACT").unwrap().i32(), Some(2));
	}

	/// A four-character name padded out to eight bytes reads as a well-formed empty block.
	#[test]
	fn a_text_payload_does_not_nest() {
		let file = read(&[nest(
			"Bind",
			&[nest("PrpS", &[block("Name", b"null\0\0\0\0")])],
		)]);

		let name = file.binders()[0]
			.find("PrpS")
			.unwrap()
			.find("Name")
			.unwrap();
		assert!(matches!(name.payload(), Payload::Bytes(_)));
		assert_eq!(name.text().as_deref(), Some("null"));
	}

	#[test]
	fn textures_are_paths() {
		let file = read(&[
			block("Tex", b"vfx/common/texture/uv_r.atex\0"),
			block("Tex", b"vfx/common/texture/fire.atex\0"),
		]);
		assert_eq!(file.textures().len(), 2);
		assert_eq!(file.textures()[1], "vfx/common/texture/fire.atex");
	}

	#[test]
	fn curve_keys_carry_a_time_and_three_floats() {
		let mut keys = Vec::new();
		for (time, kind, value) in [(0i16, 1i16, 1.0f32), (30, 0, 2.0)] {
			keys.extend(time.to_le_bytes());
			keys.extend(kind.to_le_bytes());
			keys.extend(0.5f32.to_le_bytes());
			keys.extend(0.25f32.to_le_bytes());
			keys.extend(value.to_le_bytes());
		}

		let file = read(&[nest(
			"Ptcl",
			&[nest(
				"Scl",
				&[nest(
					"X",
					&[
						block("BvPr", &integer(0)),
						block("BvPo", &integer(1)),
						block("KeyC", &integer(2)),
						block("Keys", &keys),
					],
				)],
			)],
		)]);

		let axis = file.particles()[0].find("Scl").unwrap().find("X").unwrap();
		assert_eq!(axis.find("BvPo").unwrap().i32(), Some(1));

		let keys = axis.find("Keys").unwrap().keys().unwrap();
		assert_eq!(keys.len(), 2);
		assert_eq!(keys[0].kind(), KeyKind::Linear);
		assert_eq!(keys[0].value(), 1.0);
		assert_eq!(keys[1].time(), 30);
		assert_eq!(keys[1].kind(), KeyKind::Spline);
		assert_eq!(keys[1].data(), [0.5, 0.25, 2.0]);
	}

	#[test]
	fn a_key_list_that_does_not_divide_into_keys_is_rejected() {
		let bytes = nest(
			"AVFX",
			&[block("Ver", &integer(1)), block("Keys", &[0; 20])],
		);
		assert!(matches!(
			Avfx::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	/// Item lists are written once per entry, each copy repeating the one before it.
	#[test]
	fn only_the_last_copy_of_an_item_list_is_read() {
		let file = read(&[nest(
			"Schd",
			&[
				block("ItCn", &integer(2)),
				block("TrCn", &integer(2)),
				block("Item", &item(0)),
				block("Item", &[item(0), item(1)].concat()),
				block("Trgr", &[item(0), item(1), item(10)].concat()),
				block("Trgr", &[item(0), item(1), item(10), item(11)].concat()),
			],
		)]);

		let scheduler = &file.schedulers()[0];
		assert_eq!(scheduler.items().len(), 2);
		assert_eq!(scheduler.items()[1].find("TlNo").unwrap().i32(), Some(1));

		assert_eq!(scheduler.triggers().len(), 2);
		assert_eq!(
			scheduler.triggers()[0].find("TlNo").unwrap().i32(),
			Some(10)
		);
		assert_eq!(
			scheduler.triggers()[1].find("TlNo").unwrap().i32(),
			Some(11)
		);
	}

	#[test]
	fn timeline_items_and_clips() {
		let mut clip = Vec::from(*b" DNE");
		clip.extend(integer(7));
		clip.resize(164, 0);

		let file = read(&[nest(
			"TmLn",
			&[
				block("LpSt", &integer(0)),
				block("TICn", &integer(2)),
				block("CpCn", &integer(1)),
				block("Item", &item(4)),
				block("Item", &[item(4), item(5)].concat()),
				block("Clip", &clip),
			],
		)]);

		let timeline = &file.timelines()[0];
		assert_eq!(timeline.properties().len(), 1);
		assert_eq!(timeline.properties()[0].name(), "LpSt");
		assert_eq!(timeline.items().len(), 2);
		assert_eq!(timeline.items()[0].blocks().len(), 3);

		assert_eq!(timeline.clips().len(), 1);
		assert_eq!(timeline.clips()[0].kind(), ClipKind::End);
		assert_eq!(timeline.clips()[0].integers()[0], 7);
	}

	/// The emitter list repeats the particle list before adding to it.
	#[test]
	fn emitter_particles_come_out_of_the_emitter_list() {
		let entry =
			|target: i32| [block("bEnb", &integer(1)), block("TgtB", &integer(target))].concat();

		let file = read(&[nest(
			"Emit",
			&[
				block("PrCn", &integer(1)),
				block("EmCn", &integer(1)),
				block("ItPr", &entry(3)),
				block("ItEm", &[entry(3), entry(9)].concat()),
			],
		)]);

		let emitter = &file.emitters()[0];
		assert_eq!(emitter.particles().len(), 1);
		assert_eq!(emitter.particles()[0].find("TgtB").unwrap().i32(), Some(3));
		assert_eq!(emitter.emitters().len(), 1);
		assert_eq!(emitter.emitters()[0].find("TgtB").unwrap().i32(), Some(9));
	}

	#[test]
	fn model_geometry() {
		let mut vertex = Vec::new();
		vertex.extend([0x00, 0x3c, 0, 0, 0, 0, 0, 0]);
		vertex.extend([128, 128, 255, 0]);
		vertex.extend([128, 0, 128, 128]);
		vertex.extend([1, 2, 3, 4]);
		vertex.resize(20, 0);
		vertex.extend([0x00, 0x3c, 0, 0]);
		vertex.resize(36, 0);

		let mut emit = Vec::new();
		emit.extend(1.0f32.to_le_bytes());
		emit.resize(24, 0);
		emit.extend([5, 6, 7, 8]);

		let file = read(&[nest(
			"Modl",
			&[
				block("VNum", &2u16.to_le_bytes()),
				block("VEmt", &emit),
				block("VDrw", &vertex),
				block("VIdx", &[0u16, 1, 2].map(u16::to_le_bytes).concat()),
			],
		)]);

		let model = &file.models()[0];
		assert_eq!(model.emit_vertex_numbers(), [2]);
		assert_eq!(model.emit_vertices()[0].position(), [1.0, 0.0, 0.0]);
		assert_eq!(model.emit_vertices()[0].colour(), [5, 6, 7, 8]);

		let vertex = model.vertices()[0];
		assert_eq!(vertex.position(), [1.0, 0.0, 0.0, 0.0]);
		assert_eq!(vertex.normal(), [0, 0, 127, -128]);
		assert_eq!(vertex.tangent(), [0, -128, 0, 0]);
		assert_eq!(vertex.colour(), [1, 2, 3, 4]);
		assert_eq!(vertex.uv()[0], [1.0, 0.0]);

		assert_eq!(model.triangles()[0].indices(), [0, 1, 2]);
	}

	#[test]
	fn an_array_that_does_not_divide_into_records_is_rejected() {
		let bytes = nest(
			"AVFX",
			&[
				block("Ver", &integer(1)),
				nest("Modl", &[block("VDrw", &[0; 20])]),
			],
		);
		assert!(matches!(
			Avfx::read(Cursor::new(bytes)),
			Err(Error::Invalid(..))
		));
	}

	#[test]
	fn lists_keep_their_order() {
		let file = read(&[
			nest("Ptcl", &[block("LpSt", &integer(1))]),
			nest("Ptcl", &[block("LpSt", &integer(2))]),
			nest("Efct", &[block("EfVT", &integer(0))]),
		]);

		assert_eq!(file.particles().len(), 2);
		assert_eq!(file.particles()[1].find("LpSt").unwrap().i32(), Some(2));
		assert_eq!(file.effectors().len(), 1);
	}
}
