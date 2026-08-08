//! Structs and utilities for parsing .phyb files.

use std::{
	fmt,
	io::{Read, Seek, SeekFrom},
	str,
};

use binrw::{BinRead, BinResult, Endian, binread};
use getset::{CopyGetters, Getters};

use crate::{FileStream, error::Result};

use super::File;

/// Physics for the skeleton of the same name: the shapes its bones collide with, and the simulators
/// driving chains of bones under gravity and wind.
#[binread]
#[br(little)]
#[derive(Debug, CopyGetters)]
pub struct Physics {
	/// `0x01000001` where the file carries a payload, and `0` where it does not. A file states its
	/// data type, and so carries the longer header, only where this is positive.
	#[get_copy = "pub"]
	version: i32,

	/// `3` where the file carries collision shapes and `2` where it does not, or `None` where the
	/// version is not positive. `0` is documented to place both sections at the end of the file.
	#[br(if(version > 0))]
	#[get_copy = "pub"]
	data_type: Option<u32>,

	#[br(temp)]
	collision_offset: u32,

	#[br(temp)]
	simulator_offset: u32,

	#[br(temp, restore_position, parse_with = trailer)]
	trailer: Option<Trailer>,

	// A file states it carries no collision shapes by pointing both offsets at the same place.
	#[br(
		if(collision_offset != simulator_offset),
		seek_before = SeekFrom::Start(collision_offset.into()),
	)]
	collision: Option<Collision>,

	#[br(
		parse_with = simulators,
		args(
			simulator_offset.into(),
			trailer.as_ref().map(|trailer| trailer.content_end),
		),
	)]
	simulators: Vec<Simulator>,

	#[br(calc = trailer.map(|trailer| trailer.payload))]
	extended: Option<Vec<u8>>,
}

impl Physics {
	/// The shapes bones collide with, where the file carries any.
	pub fn collision(&self) -> Option<&Collision> {
		self.collision.as_ref()
	}

	/// The simulators the file carries.
	pub fn simulators(&self) -> &[Simulator] {
		&self.simulators
	}

	/// The extended physics block Dawntrail added, a FlatBuffer carrying the `EPHB` file identifier,
	/// where the file carries one. Its tables are unidentified.
	pub fn extended(&self) -> Option<&[u8]> {
		self.extended.as_deref()
	}
}

impl File for Physics {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// The shapes a skeleton's bones collide with.
#[binread]
#[br(little)]
#[derive(Debug, Getters)]
#[get = "pub"]
pub struct Collision {
	#[br(temp)]
	counts: [u8; 5],

	// The three bytes after the counts are uninitialised filler.
	#[br(pad_before = 3, count = counts[0])]
	capsules: Vec<Capsule>,

	#[br(count = counts[1])]
	ellipsoids: Vec<Ellipsoid>,

	#[br(count = counts[2])]
	normal_planes: Vec<NormalPlane>,

	#[br(count = counts[3])]
	three_point_planes: Vec<ThreePointPlane>,

	#[br(count = counts[4])]
	spheres: Vec<Sphere>,
}

/// A sphere swept between two bones.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Capsule {
	name: Name,

	start_bone: Name,

	end_bone: Name,

	/// In the start bone's space.
	start_offset: [f32; 3],

	/// In the end bone's space.
	end_offset: [f32; 3],

	radius: f32,
}

/// An ellipsoid around a bone. Its four offsets are unidentified.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Ellipsoid {
	name: Name,

	bone: Name,

	offsets: [[f32; 3]; 4],

	radius: f32,
}

/// A plane stated by a point on a bone and a normal.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct NormalPlane {
	name: Name,

	bone: Name,

	bone_offset: [f32; 3],

	normal: [f32; 3],

	thickness: f32,
}

/// A plane whose leading four vectors are unidentified.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct ThreePointPlane {
	name: Name,

	bone: Name,

	unknown_a: [[f32; 4]; 4],

	bone_offset: [f32; 3],

	unknown_b: [f32; 3],

	unknown_c: [f32; 3],

	thickness: f32,
}

/// A sphere around a point on a bone.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Sphere {
	name: Name,

	bone: Name,

	bone_offset: [f32; 3],

	thickness: f32,
}

/// A set of bone chains sharing gravity, wind and collision settings.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug, Getters, CopyGetters)]
pub struct Simulator {
	#[br(temp)]
	counts: [u8; 8],

	#[get_copy = "pub"]
	gravity: [f32; 3],

	#[get_copy = "pub"]
	wind: [f32; 3],

	/// How many times a step resolves the chains' length constraints.
	#[get_copy = "pub"]
	constraint_loop: u16,

	/// How many times a step resolves collisions.
	#[get_copy = "pub"]
	collision_loop: u16,

	#[br(map = Flags)]
	#[get_copy = "pub"]
	flags: Flags,

	/// Row of the `PhysicsGroup` sheet this simulator belongs to.
	// The two bytes after the group are uninitialised filler.
	#[br(pad_after = 2)]
	#[get_copy = "pub"]
	group: u8,

	#[br(temp)]
	offsets: [u32; 8],

	/// Shapes every chain of this simulator collides with.
	#[br(restore_position, parse_with = list, args(base, offsets[0], counts[0].into(), ()))]
	#[get = "pub"]
	collision_objects: Vec<CollisionData>,

	/// Shapes this simulator's connectors collide with.
	#[br(restore_position, parse_with = list, args(base, offsets[1], counts[1].into(), ()))]
	#[get = "pub"]
	collision_connectors: Vec<CollisionData>,

	/// The chains this simulator drives.
	#[br(restore_position, parse_with = list, args(base, offsets[2], counts[2].into(), (base,)))]
	#[get = "pub"]
	chains: Vec<Chain>,

	/// Links between chains, colliding along their length.
	#[br(restore_position, parse_with = list, args(base, offsets[3], counts[3].into(), ()))]
	#[get = "pub"]
	connectors: Vec<Connector>,

	/// Bones pulling nodes towards themselves.
	#[br(restore_position, parse_with = list, args(base, offsets[4], counts[4].into(), ()))]
	#[get = "pub"]
	attracts: Vec<Attract>,

	/// Nodes held to a bone rather than simulated.
	#[br(restore_position, parse_with = list, args(base, offsets[5], counts[5].into(), ()))]
	#[get = "pub"]
	pins: Vec<Pin>,

	/// Springs holding pairs of nodes apart.
	#[br(restore_position, parse_with = list, args(base, offsets[6], counts[6].into(), ()))]
	#[get = "pub"]
	springs: Vec<Spring>,

	/// Nodes aligned against a collision shape once the step is done.
	#[br(restore_position, parse_with = list, args(base, offsets[7], counts[7].into(), ()))]
	#[get = "pub"]
	post_alignments: Vec<PostAlignment>,
}

/// A shape from the collision section, and the side of it bones are held to.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct CollisionData {
	name: Name,

	collision_type: CollisionType,
}

/// Which side of a collision shape bones are held to.
#[allow(missing_docs)]
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionType {
	#[br(magic = 0i32)]
	Both,

	#[br(magic = 1i32)]
	Outside,

	#[br(magic = 2i32)]
	Inside,

	Unknown(i32),
}

/// A chain of bones simulated as one.
#[binread]
#[br(little, import(base: u64))]
#[derive(Debug, Getters, CopyGetters)]
pub struct Chain {
	#[br(temp)]
	collision_count: u16,

	#[br(temp)]
	node_count: u16,

	#[get_copy = "pub"]
	dampening: f32,

	#[get_copy = "pub"]
	max_speed: f32,

	#[get_copy = "pub"]
	friction: f32,

	#[get_copy = "pub"]
	collision_dampening: f32,

	#[get_copy = "pub"]
	repulsion_strength: f32,

	/// Where the chain ends, in the last node's bone space.
	#[get_copy = "pub"]
	last_bone_offset: [f32; 3],

	#[get_copy = "pub"]
	chain_type: ChainType,

	#[br(temp)]
	collision_offset: u32,

	#[br(temp)]
	node_offset: u32,

	/// Shapes this chain collides with, on top of its simulator's.
	#[br(restore_position, parse_with = list, args(base, collision_offset, collision_count.into(), ()))]
	#[get = "pub"]
	collisions: Vec<CollisionData>,

	/// The bones of the chain, in the order the file states them.
	#[br(restore_position, parse_with = list, args(base, node_offset, node_count.into(), ()))]
	#[get = "pub"]
	nodes: Vec<Node>,
}

/// The shape a chain's nodes are treated as.
#[allow(missing_docs)]
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainType {
	#[br(magic = 0u32)]
	Sphere,

	#[br(magic = 1u32)]
	Capsule,

	Unknown(u32),
}

/// One bone of a chain.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Node {
	bone: Name,

	radius: f32,

	/// How much the animated pose pulls the node back towards itself.
	attract_by_animation: f32,

	wind_scale: f32,

	gravity_scale: f32,

	/// How far the node may swing from its cone axis, in radians.
	cone_max_angle: f32,

	cone_axis_offset: [f32; 3],

	constraint_plane_normal: [f32; 3],

	/// Which of the simulator's collision objects apply, by bit position.
	collision_flags: u32,

	continuous_collision_flags: u32,
}

/// A collided link between two chains' nodes.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Connector {
	chain_ids: [u16; 2],

	node_ids: [u16; 2],

	collision_radius: f32,

	friction: f32,

	dampening: f32,

	repulsion: f32,

	/// Which of the simulator's collision connectors apply, by bit position.
	collision_flags: u32,

	continuous_collision_flags: u32,
}

/// A bone pulling one node towards it.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Attract {
	bone: Name,

	bone_offset: [f32; 3],

	chain_id: u16,

	node_id: u16,

	stiffness: f32,
}

/// A node held to a bone.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Pin {
	bone: Name,

	bone_offset: [f32; 3],

	chain_id: u16,

	node_id: u16,
}

/// A spring between two chains' nodes.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Spring {
	chain_ids: [u16; 2],

	node_ids: [u16; 2],

	stretch_stiffness: f32,

	compress_stiffness: f32,
}

/// A node and the collision shape it is aligned against once the step is done.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct PostAlignment {
	collision_name: Name,

	chain_id: u16,

	node_id: u16,
}

/// The switches a simulator declares.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Flags(u8);

impl Flags {
	const SIMULATING: u8 = 0x01;
	const COLLISIONS_HANDLED: u8 = 0x02;
	const CONTINUOUS_COLLISIONS: u8 = 0x04;
	const USING_GROUND_PLANE: u8 = 0x08;
	const FIXED_LENGTH: u8 = 0x10;

	/// The flag byte as written.
	pub fn bits(&self) -> u8 {
		self.0
	}

	/// Whether the simulator runs at all.
	pub fn simulating(&self) -> bool {
		self.0 & Self::SIMULATING != 0
	}

	/// Whether the simulator's collision shapes are honoured.
	pub fn collisions_handled(&self) -> bool {
		self.0 & Self::COLLISIONS_HANDLED != 0
	}

	/// Whether collisions are swept across the step rather than tested at its end.
	pub fn continuous_collisions(&self) -> bool {
		self.0 & Self::CONTINUOUS_COLLISIONS != 0
	}

	/// Whether the ground is treated as a collision plane.
	pub fn using_ground_plane(&self) -> bool {
		self.0 & Self::USING_GROUND_PLANE != 0
	}

	/// Whether the chains hold their rest length.
	pub fn fixed_length(&self) -> bool {
		self.0 & Self::FIXED_LENGTH != 0
	}
}

impl fmt::Debug for Flags {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let names = [
			(Self::SIMULATING, "simulating"),
			(Self::COLLISIONS_HANDLED, "collisions_handled"),
			(Self::CONTINUOUS_COLLISIONS, "continuous_collisions"),
			(Self::USING_GROUND_PLANE, "using_ground_plane"),
			(Self::FIXED_LENGTH, "fixed_length"),
		];
		let mut list = f.debug_list();
		for (mask, name) in names {
			if self.0 & mask != 0 {
				list.entry(&name);
			}
		}
		list.finish()
	}
}

/// A fixed-width name. Buffers carry filler past the terminator.
#[binread]
#[br(little)]
#[derive(Clone, Copy)]
pub struct Name([u8; 32]);

impl Name {
	/// The name as written, up to but not including its terminator. A handful of names are Shift-JIS
	/// rather than UTF-8, and this is the only way to reach those; it is also the key to match a
	/// [`CollisionData`] against the collision section by.
	pub fn as_bytes(&self) -> &[u8] {
		let end = self
			.0
			.iter()
			.position(|byte| *byte == 0)
			.unwrap_or(self.0.len());
		&self.0[..end]
	}

	/// The name as written, or `None` if it is not UTF-8.
	pub fn as_str(&self) -> Option<&str> {
		str::from_utf8(self.as_bytes()).ok()
	}
}

impl fmt::Debug for Name {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self.as_str() {
			Some(text) => write!(f, "{text:?}"),
			None => write!(f, "{:?}", self.0),
		}
	}
}

/// Reads a list of `count` items from `base + offset`. Offsets naming an empty list hold filler, so
/// they are only followed where the count is nonzero.
fn list<T, A, R>(
	reader: &mut R,
	endian: Endian,
	(base, offset, count, args): (u64, u32, usize, A),
) -> BinResult<Vec<T>>
where
	T: for<'a> BinRead<Args<'a> = A>,
	A: Clone,
	R: Read + Seek,
{
	if count == 0 {
		return Ok(Vec::new());
	}

	reader.seek(SeekFrom::Start(base + u64::from(offset)))?;
	let mut items = Vec::with_capacity(count);
	for _ in 0..count {
		items.push(T::read_options(reader, endian, args.clone())?);
	}
	Ok(items)
}

/// Reads the simulator section, which a file states as empty by pointing past its own content.
fn simulators<R: Read + Seek>(
	reader: &mut R,
	endian: Endian,
	(offset, content_end): (u64, Option<u64>),
) -> BinResult<Vec<Simulator>> {
	let end = match content_end {
		Some(end) => end,
		None => reader.seek(SeekFrom::End(0))?,
	};
	if offset >= end {
		return Ok(Vec::new());
	}

	reader.seek(SeekFrom::Start(offset))?;
	let count = u32::read_options(reader, endian, ())?;

	// Every offset within the section, including each chain's, is relative to here.
	let base = offset + 4;
	(0..count)
		.map(|_| Simulator::read_options(reader, endian, (base,)))
		.collect()
}

/// The pack trailer added in Dawntrail, and where the file's own content ends.
struct Trailer {
	content_end: u64,
	payload: Vec<u8>,
}

/// A pack header, which both the trailer and the block it holds lead with. The skipped field is the
/// pack's own version, which is `1` throughout.
#[binread]
#[br(little)]
struct PackHeader {
	kind: u32,

	#[br(pad_before = 2)]
	count: u16,

	prior_offset: i64,
}

const PACK: u32 = u32::from_le_bytes(*b"PACK");
const EPHB: u32 = u32::from_le_bytes(*b"EPHB");

/// Reads the pack trailer, where the file carries one. Content that happens to end in the magic is
/// ruled out by the sizes the trailer states.
fn trailer<R: Read + Seek>(reader: &mut R, endian: Endian, _: ()) -> BinResult<Option<Trailer>> {
	let length = reader.seek(SeekFrom::End(0))?;
	let Some(footer_at) = length.checked_sub(24) else {
		return Ok(None);
	};

	reader.seek(SeekFrom::Start(footer_at))?;
	let footer = PackHeader::read_options(reader, endian, ())?;
	let total_size = u64::read_options(reader, endian, ())?;
	if footer.kind != PACK || footer.count == 0 || total_size < 40 || total_size > length {
		return Ok(None);
	}

	// The size covers the block's leading alignment padding as well as the block itself, so it is
	// the content that ends there; the footer names the block's own start.
	let content_end = length - total_size;
	let Some(at) = footer_at.checked_add_signed(footer.prior_offset) else {
		return Ok(None);
	};
	if at < content_end || at + 16 > footer_at {
		return Ok(None);
	}

	reader.seek(SeekFrom::Start(at))?;
	if PackHeader::read_options(reader, endian, ())?.kind != EPHB {
		return Ok(None);
	}

	let mut payload = vec![0; usize::try_from(footer_at - (at + 16)).unwrap()];
	reader.read_exact(&mut payload)?;
	Ok(Some(Trailer {
		content_end,
		payload,
	}))
}

#[cfg(test)]
mod test {
	use std::{
		f32::consts::FRAC_PI_4,
		io::{self, Cursor},
	};

	use crate::{error::Error, file::File};

	use super::{ChainType, CollisionType, Physics};

	/// A name buffer, filled past the terminator with the game's own filler byte.
	fn name(text: &[u8]) -> Vec<u8> {
		let mut buffer = vec![0xfe; 32];
		buffer[..text.len()].copy_from_slice(text);
		buffer[text.len()] = 0;
		buffer
	}

	fn floats(values: &[f32]) -> Vec<u8> {
		values
			.iter()
			.flat_map(|value| value.to_le_bytes())
			.collect()
	}

	fn capsule(id: &[u8], start: &[u8], end: &[u8], radius: f32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(name(start));
		bytes.extend(name(end));
		bytes.extend(floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, radius]));
		bytes
	}

	fn ellipsoid(id: &[u8], radius: f32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(name(b"j_ude_a_l"));
		bytes.extend(floats(&[0.0; 12]));
		bytes.extend(radius.to_le_bytes());
		bytes
	}

	fn normal_plane(id: &[u8], thickness: f32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(name(b"j_sebo_c"));
		bytes.extend(floats(&[0.1, 0.2, 0.3, 0.0, 1.0, 0.0, thickness]));
		bytes
	}

	fn three_point_plane(id: &[u8], thickness: f32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(name(b"j_kosi"));
		bytes.extend(floats(&[7.0; 16]));
		bytes.extend(floats(&[0.4, 0.5, 0.6]));
		bytes.extend(floats(&[0.0; 6]));
		bytes.extend(thickness.to_le_bytes());
		bytes
	}

	fn sphere(id: &[u8], bone: &[u8], thickness: f32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(name(bone));
		bytes.extend(floats(&[9.0, 8.0, 7.0, thickness]));
		bytes
	}

	/// The five record lists of a collision section, in the order the counts state them.
	fn collision(lists: [&[Vec<u8>]; 5]) -> Vec<u8> {
		let mut bytes: Vec<u8> = lists
			.iter()
			.map(|list| u8::try_from(list.len()).unwrap())
			.collect();
		// The game leaves the three bytes after the counts uninitialised.
		bytes.extend([0xcc; 3]);
		bytes.extend(lists.into_iter().flatten().flatten());
		bytes
	}

	fn collision_data(id: &[u8], kind: i32) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(kind.to_le_bytes());
		bytes
	}

	fn node(bone: &[u8], radius: f32, collision_flags: u32) -> Vec<u8> {
		let mut bytes = name(bone);
		bytes.extend(floats(&[radius, 0.25, 0.5, 1.0, FRAC_PI_4]));
		bytes.extend(floats(&[0.0, 1.0, 0.0, 1.0, 0.0, 0.0]));
		bytes.extend(collision_flags.to_le_bytes());
		bytes.extend(0u32.to_le_bytes());
		bytes
	}

	fn attract(bone: &[u8], chain: u16, node: u16, stiffness: f32) -> Vec<u8> {
		let mut bytes = name(bone);
		bytes.extend(floats(&[0.0; 3]));
		bytes.extend(chain.to_le_bytes());
		bytes.extend(node.to_le_bytes());
		bytes.extend(stiffness.to_le_bytes());
		bytes
	}

	fn pin(bone: &[u8], chain: u16, node: u16) -> Vec<u8> {
		let mut bytes = name(bone);
		bytes.extend(floats(&[0.0; 3]));
		bytes.extend(chain.to_le_bytes());
		bytes.extend(node.to_le_bytes());
		bytes
	}

	fn connector(chains: [u16; 2], nodes: [u16; 2], radius: f32) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(chains.iter().flat_map(|id| id.to_le_bytes()));
		bytes.extend(nodes.iter().flat_map(|id| id.to_le_bytes()));
		bytes.extend(floats(&[radius, 0.1, 0.2, 0.3]));
		bytes.extend(floats(&[0.0; 2]));
		bytes
	}

	fn spring(chains: [u16; 2], nodes: [u16; 2], stretch: f32) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(chains.iter().flat_map(|id| id.to_le_bytes()));
		bytes.extend(nodes.iter().flat_map(|id| id.to_le_bytes()));
		bytes.extend(floats(&[stretch, 0.9]));
		bytes
	}

	fn post_alignment(id: &[u8], chain: u16, node: u16) -> Vec<u8> {
		let mut bytes = name(id);
		bytes.extend(chain.to_le_bytes());
		bytes.extend(node.to_le_bytes());
		bytes
	}

	#[derive(Default)]
	struct Chain {
		dampening: f32,
		chain_type: u32,
		collisions: Vec<Vec<u8>>,
		nodes: Vec<Vec<u8>>,
	}

	#[derive(Default)]
	struct Sim {
		gravity: [f32; 3],
		flags: u8,
		group: u8,
		collision_objects: Vec<Vec<u8>>,
		collision_connectors: Vec<Vec<u8>>,
		chains: Vec<Chain>,
		connectors: Vec<Vec<u8>>,
		attracts: Vec<Vec<u8>>,
		pins: Vec<Vec<u8>>,
		springs: Vec<Vec<u8>>,
		post_alignments: Vec<Vec<u8>>,
	}

	/// Lays out a simulator section: the count, then one 72-byte header per simulator, then the lists
	/// they point at, each stated as an offset from the byte after the count.
	fn simulation(sims: &[Sim]) -> Vec<u8> {
		let base = 4 + 72 * sims.len();
		let mut body = Vec::new();
		let mut headers = Vec::new();

		// The game states an empty list as a zero offset, except post alignments, whose slot is left
		// holding uninitialised filler.
		let place = |body: &mut Vec<u8>, records: &[Vec<u8>], empty: u32| match records.is_empty() {
			true => empty,
			false => {
				let offset = u32::try_from(base + body.len() - 4).unwrap();
				body.extend(records.iter().flatten());
				offset
			}
		};

		for sim in sims {
			let mut chains = Vec::new();
			for chain in &sim.chains {
				// A chain's own lists are placed before the record that names them.
				let collisions = place(&mut body, &chain.collisions, 0);
				let nodes = place(&mut body, &chain.nodes, 0);

				let mut record = Vec::new();
				record.extend(u16::try_from(chain.collisions.len()).unwrap().to_le_bytes());
				record.extend(u16::try_from(chain.nodes.len()).unwrap().to_le_bytes());
				record.extend(floats(&[chain.dampening, 20.0, 0.5, 0.1, 0.2]));
				record.extend(floats(&[0.0, 0.0, -1.0]));
				record.extend(chain.chain_type.to_le_bytes());
				record.extend(collisions.to_le_bytes());
				record.extend(nodes.to_le_bytes());
				chains.push(record);
			}

			let lists = [
				&sim.collision_objects,
				&sim.collision_connectors,
				&chains,
				&sim.connectors,
				&sim.attracts,
				&sim.pins,
				&sim.springs,
				&sim.post_alignments,
			];
			headers.extend(lists.map(|list| u8::try_from(list.len()).unwrap()));
			headers.extend(floats(&sim.gravity));
			headers.extend(floats(&[0.0, 0.0, 0.0]));
			headers.extend(2u16.to_le_bytes());
			headers.extend(1u16.to_le_bytes());
			headers.extend([sim.flags, sim.group, 0xcc, 0xcc]);
			for (index, list) in lists.into_iter().enumerate() {
				let empty = match index {
					7 => 0xcccc_cccc,
					_ => 0,
				};
				headers.extend(place(&mut body, list, empty).to_le_bytes());
			}
		}

		let mut bytes = u32::try_from(sims.len()).unwrap().to_le_bytes().to_vec();
		bytes.extend(headers);
		bytes.extend(body);
		bytes
	}

	/// Appends a pack trailer holding `payload`, aligning the block to eight as the game does.
	fn extended(bytes: &mut Vec<u8>, payload: &[u8]) {
		let content_end = bytes.len();
		bytes.resize(content_end.next_multiple_of(8), 0);

		bytes.extend(b"EPHB");
		bytes.extend(1u16.to_le_bytes());
		bytes.extend(0u16.to_le_bytes());
		bytes.extend(i64::try_from(payload.len()).unwrap().to_le_bytes());
		bytes.extend(payload);

		bytes.extend(b"PACK");
		bytes.extend(1u16.to_le_bytes());
		bytes.extend(1u16.to_le_bytes());
		bytes.extend((-i64::try_from(16 + payload.len()).unwrap()).to_le_bytes());
		let total = bytes.len() + 8 - content_end;
		bytes.extend(u64::try_from(total).unwrap().to_le_bytes());
	}

	fn phyb(data_type: u32, collision: &[u8], simulation: &[u8]) -> Vec<u8> {
		let mut bytes = 0x0100_0001u32.to_le_bytes().to_vec();
		bytes.extend(data_type.to_le_bytes());
		bytes.extend(16u32.to_le_bytes());
		bytes.extend(u32::try_from(16 + collision.len()).unwrap().to_le_bytes());
		bytes.extend(collision);
		bytes.extend(simulation);
		bytes
	}

	fn one_chain() -> Vec<Sim> {
		vec![Sim {
			gravity: [0.0, 0.0, -9.8],
			flags: 0x13,
			group: 9,
			chains: vec![Chain {
				dampening: 0.75,
				nodes: vec![node(b"j_kami_a", 0.05, 1), node(b"j_kami_b", 0.04, 2)],
				..Default::default()
			}],
			..Default::default()
		}]
	}

	#[test]
	fn empty() {
		assert!(matches!(
			Physics::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	/// The whole of a version-zero file: no data type, and both offsets at its end. A reader taking
	/// the data type unconditionally runs off the end of these twelve bytes.
	#[test]
	fn the_short_header_states_no_sections() {
		let mut bytes = 0u32.to_le_bytes().to_vec();
		bytes.extend(12u32.to_le_bytes());
		bytes.extend(12u32.to_le_bytes());

		let file = Physics::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.version(), 0);
		assert_eq!(file.data_type(), None);
		assert!(file.collision().is_none());
		assert!(file.simulators().is_empty());
		assert!(file.extended().is_none());
	}

	#[test]
	fn reads_collision_shapes() {
		let bytes = phyb(
			3,
			&collision([
				&[capsule(b"kubi", b"j_kubi", b"j_kao", 0.06)],
				&[ellipsoid(b"ude", 0.11)],
				&[normal_plane(b"sebo", 0.02)],
				&[three_point_plane(b"kosi", 0.03)],
				&[
					sphere(b"mune_l", b"j_mune_l", 0.09),
					sphere(b"mune_r", b"j_mune_r", 0.1),
				],
			]),
			&simulation(&one_chain()),
		);

		let file = Physics::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.version(), 0x0100_0001);
		assert_eq!(file.data_type(), Some(3));

		let collision = file.collision().unwrap();
		let capsule = collision.capsules()[0];
		assert_eq!(capsule.name().as_str(), Some("kubi"));
		assert_eq!(capsule.start_bone().as_str(), Some("j_kubi"));
		assert_eq!(capsule.end_bone().as_str(), Some("j_kao"));
		assert_eq!(capsule.start_offset(), [1.0, 2.0, 3.0]);
		assert_eq!(capsule.radius(), 0.06);

		assert_eq!(collision.ellipsoids()[0].radius(), 0.11);
		assert_eq!(collision.normal_planes()[0].normal(), [0.0, 1.0, 0.0]);
		assert_eq!(collision.normal_planes()[0].thickness(), 0.02);

		let plane = collision.three_point_planes()[0];
		assert_eq!(plane.name().as_str(), Some("kosi"));
		assert_eq!(plane.bone_offset(), [0.4, 0.5, 0.6]);
		assert_eq!(plane.thickness(), 0.03);

		// Reached only if every stride before it is right.
		assert_eq!(collision.spheres().len(), 2);
		assert_eq!(collision.spheres()[1].name().as_str(), Some("mune_r"));
		assert_eq!(collision.spheres()[1].bone().as_str(), Some("j_mune_r"));
		assert_eq!(collision.spheres()[1].thickness(), 0.1);
	}

	/// Both offsets meeting is how a file states it carries no collision section at all.
	#[test]
	fn equal_offsets_state_no_collision() {
		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&one_chain())))).unwrap();
		assert!(file.collision().is_none());
		assert_eq!(file.simulators().len(), 1);
	}

	#[test]
	fn reads_a_simulator_and_its_lists() {
		let sims = vec![Sim {
			gravity: [0.0, 0.0, -9.8],
			flags: 0x1b,
			group: 22,
			collision_objects: vec![
				collision_data(b"kubi", 1),
				collision_data(b"mune_l", 2),
				collision_data(b"mune_r", 0),
			],
			collision_connectors: vec![collision_data(b"sebo", 1)],
			chains: vec![
				Chain {
					dampening: 0.75,
					chain_type: 1,
					collisions: vec![collision_data(b"kubi", 1)],
					nodes: vec![node(b"j_kami_a", 0.05, 1), node(b"j_kami_b", 0.04, 6)],
					..Default::default()
				},
				Chain {
					dampening: 0.5,
					nodes: vec![node(b"j_sk_b_a_l", 0.02, 0)],
					..Default::default()
				},
			],
			connectors: vec![connector([0, 1], [1, 0], 0.03)],
			attracts: vec![attract(b"j_kubi", 0, 1, 0.4)],
			pins: vec![pin(b"j_kao", 1, 0)],
			springs: vec![spring([0, 1], [0, 0], 0.6)],
			post_alignments: vec![post_alignment(b"kubi", 0, 1)],
			..Default::default()
		}];

		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&sims)))).unwrap();
		let simulator = &file.simulators()[0];

		assert_eq!(simulator.gravity(), [0.0, 0.0, -9.8]);
		assert_eq!(simulator.constraint_loop(), 2);
		assert_eq!(simulator.collision_loop(), 1);
		assert_eq!(simulator.group(), 22);
		assert!(simulator.flags().simulating());
		assert!(simulator.flags().collisions_handled());
		assert!(!simulator.flags().continuous_collisions());
		assert!(simulator.flags().using_ground_plane());
		assert!(simulator.flags().fixed_length());

		assert_eq!(simulator.collision_objects().len(), 3);
		assert_eq!(
			simulator.collision_objects()[1].collision_type(),
			CollisionType::Inside
		);
		assert_eq!(
			simulator.collision_objects()[2].collision_type(),
			CollisionType::Both
		);
		assert_eq!(
			simulator.collision_connectors()[0].name().as_str(),
			Some("sebo")
		);

		assert_eq!(simulator.chains().len(), 2);
		let chain = &simulator.chains()[0];
		assert_eq!(chain.dampening(), 0.75);
		assert_eq!(chain.max_speed(), 20.0);
		assert_eq!(chain.chain_type(), ChainType::Capsule);
		assert_eq!(chain.last_bone_offset(), [0.0, 0.0, -1.0]);
		assert_eq!(chain.collisions().len(), 1);
		assert_eq!(chain.nodes().len(), 2);
		assert_eq!(chain.nodes()[1].bone().as_str(), Some("j_kami_b"));
		assert_eq!(chain.nodes()[1].radius(), 0.04);
		assert_eq!(chain.nodes()[1].cone_max_angle(), FRAC_PI_4);
		assert_eq!(chain.nodes()[1].collision_flags(), 6);
		assert_eq!(simulator.chains()[1].chain_type(), ChainType::Sphere);
		assert_eq!(
			simulator.chains()[1].nodes()[0].bone().as_str(),
			Some("j_sk_b_a_l")
		);

		assert_eq!(simulator.connectors()[0].chain_ids(), [0, 1]);
		assert_eq!(simulator.connectors()[0].node_ids(), [1, 0]);
		assert_eq!(simulator.connectors()[0].collision_radius(), 0.03);
		assert_eq!(simulator.attracts()[0].bone().as_str(), Some("j_kubi"));
		assert_eq!(simulator.attracts()[0].stiffness(), 0.4);
		assert_eq!(simulator.pins()[0].bone().as_str(), Some("j_kao"));
		assert_eq!(simulator.pins()[0].chain_id(), 1);
		assert_eq!(simulator.springs()[0].stretch_stiffness(), 0.6);
		assert_eq!(simulator.springs()[0].compress_stiffness(), 0.9);
		assert_eq!(
			simulator.post_alignments()[0].collision_name().as_str(),
			Some("kubi")
		);
	}

	/// Two simulators' headers run back to back, and every offset in either of them is stated from
	/// the byte after the section's count rather than from the header it appears in.
	#[test]
	fn every_offset_is_stated_from_one_base() {
		let sims = vec![
			Sim {
				group: 3,
				chains: vec![Chain {
					nodes: vec![node(b"j_kami_a", 0.05, 0)],
					..Default::default()
				}],
				..Default::default()
			},
			Sim {
				group: 4,
				collision_objects: vec![collision_data(b"kubi", 1)],
				chains: vec![Chain {
					nodes: vec![node(b"j_kami_b", 0.04, 0), node(b"j_kami_c", 0.03, 0)],
					..Default::default()
				}],
				..Default::default()
			},
		];

		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&sims)))).unwrap();
		assert_eq!(file.simulators().len(), 2);
		assert_eq!(file.simulators()[0].group(), 3);
		assert_eq!(
			file.simulators()[0].chains()[0].nodes()[0].bone().as_str(),
			Some("j_kami_a")
		);
		assert_eq!(file.simulators()[1].group(), 4);
		assert_eq!(file.simulators()[1].chains()[0].nodes().len(), 2);
		assert_eq!(
			file.simulators()[1].chains()[0].nodes()[1].bone().as_str(),
			Some("j_kami_c")
		);
	}

	/// The post alignment slot of a simulator that has none holds uninitialised filler, which would
	/// seek far past the end of the file if it were followed.
	#[test]
	fn a_slot_naming_no_records_is_not_followed() {
		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&one_chain())))).unwrap();
		assert!(file.simulators()[0].post_alignments().is_empty());
		assert!(file.simulators()[0].pins().is_empty());
	}

	#[test]
	fn unknown_discriminants_are_carried_through() {
		let sims = vec![Sim {
			collision_objects: vec![collision_data(b"kubi", 129)],
			chains: vec![Chain {
				chain_type: 7,
				nodes: vec![node(b"j_kami_a", 0.05, 0)],
				..Default::default()
			}],
			..Default::default()
		}];

		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&sims)))).unwrap();
		let simulator = &file.simulators()[0];
		assert_eq!(
			simulator.collision_objects()[0].collision_type(),
			CollisionType::Unknown(129)
		);
		assert_eq!(simulator.chains()[0].chain_type(), ChainType::Unknown(7));
	}

	/// A name is a fixed-width buffer, so its filler is not part of it, and nothing promises the
	/// bytes before the terminator are UTF-8: a handful of shipped names are Shift-JIS.
	#[test]
	fn names_stop_at_their_terminator() {
		let mut shift_jis = Vec::from(*b"ude_a_");
		shift_jis.extend([0x82, 0x92]);
		let sims = vec![Sim {
			collision_objects: vec![
				collision_data(&shift_jis, 1),
				collision_data(b"", 1),
				collision_data(b"a_name_filling_all_but_one_byte", 1),
			],
			..Default::default()
		}];

		let file = Physics::read(Cursor::new(phyb(2, &[], &simulation(&sims)))).unwrap();
		let shapes = file.simulators()[0].collision_objects();
		assert_eq!(shapes[0].name().as_str(), None);
		assert_eq!(shapes[0].name().as_bytes(), shift_jis);
		assert_eq!(shapes[1].name().as_str(), Some(""));
		assert_eq!(
			shapes[2].name().as_str(),
			Some("a_name_filling_all_but_one_byte")
		);
	}

	/// The trailer's size covers the alignment padding before its block, so the block starts past
	/// where subtracting the size lands. Nine bytes of payload leave the content ending off eight.
	#[test]
	fn the_trailer_block_is_found_past_the_padding() {
		for payload in [vec![7; 16], vec![7; 9]] {
			let mut bytes = phyb(2, &[], &simulation(&one_chain()));
			let content_end = bytes.len();
			extended(&mut bytes, &payload);
			assert_ne!(bytes.len() - content_end, 40 + payload.len() - 8);

			let file = Physics::read(Cursor::new(bytes)).unwrap();
			assert_eq!(file.extended(), Some(payload.as_slice()));
			assert_eq!(file.simulators().len(), 1);
		}
	}

	/// A file with a trailer states an empty simulator section by pointing at the block, not at the
	/// end of the file. A reader taking the simulator count unconditionally reads the magic as one.
	#[test]
	fn an_empty_simulator_section_stops_at_the_trailer() {
		let shapes = collision([&[], &[], &[], &[], &[sphere(b"kubi", b"j_kubi", 0.09)]]);
		let mut bytes = phyb(3, &shapes, &[]);
		extended(&mut bytes, &[7; 24]);

		let file = Physics::read(Cursor::new(bytes)).unwrap();
		assert_eq!(file.collision().unwrap().spheres().len(), 1);
		assert!(file.simulators().is_empty());
		assert_eq!(file.extended(), Some([7; 24].as_slice()));
	}

	#[test]
	fn an_empty_simulator_section_stops_at_the_end_of_the_file() {
		let shapes = collision([&[], &[], &[], &[], &[sphere(b"kubi", b"j_kubi", 0.09)]]);
		let file = Physics::read(Cursor::new(phyb(3, &shapes, &[]))).unwrap();
		assert_eq!(file.collision().unwrap().spheres().len(), 1);
		assert!(file.simulators().is_empty());
		assert!(file.extended().is_none());
	}

	/// Content ending in the magic is not a trailer: the sizes it would have to state do not hold.
	#[test]
	fn content_ending_in_the_magic_is_not_a_trailer() {
		let mut bytes = phyb(2, &[], &simulation(&one_chain()));
		bytes.extend(b"PACK");
		bytes.extend(1u16.to_le_bytes());
		bytes.extend(1u16.to_le_bytes());
		bytes.extend((-64i64).to_le_bytes());
		bytes.extend(u64::MAX.to_le_bytes());

		let file = Physics::read(Cursor::new(bytes)).unwrap();
		assert!(file.extended().is_none());
		assert_eq!(file.simulators().len(), 1);
	}

	#[test]
	fn truncated() {
		let mut bytes = phyb(2, &[], &simulation(&one_chain()));
		bytes.truncate(bytes.len() - 4);
		assert!(matches!(
			Physics::read(Cursor::new(bytes)),
			Err(Error::Resource(_))
		));
	}
}
