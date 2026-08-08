use getset::CopyGetters;

use super::block::{Block, find};

/// One entry of a scheduler, timeline or emitter list, held as the run of blocks describing it.
#[derive(Debug)]
pub struct Item {
	blocks: Vec<Block>,
}

impl Item {
	/// The blocks describing this entry, in file order.
	pub fn blocks(&self) -> &[Block] {
		&self.blocks
	}

	/// The first block carrying `name`.
	pub fn find(&self, name: &str) -> Option<&Block> {
		find(&self.blocks, name)
	}
}

/// The entries of an item list.
///
/// A list is written once per entry, each copy repeating everything the copy before it held, so
/// only the last is read. Within it, entries run together and a repeat of the leading tag starts
/// the next one.
fn items(container: Option<Block>) -> Vec<Item> {
	let Some(container) = container else {
		return Vec::new();
	};

	let blocks = container.into_blocks();
	let Some(leading) = blocks.first().map(Block::name) else {
		return Vec::new();
	};

	let mut items = Vec::<Item>::new();
	for block in blocks {
		if block.name() == leading || items.is_empty() {
			items.push(Item { blocks: Vec::new() });
		}
		items.last_mut().unwrap().blocks.push(block);
	}
	items
}

/// What drives an effect: which timelines it runs, and when each of them starts.
///
/// Each entry carries `bEna`, `StTm` and `TlNo`, the last indexing [`Avfx::timelines`].
///
/// [`Avfx::timelines`]: super::Avfx::timelines
#[derive(Debug)]
pub struct Scheduler {
	properties: Vec<Block>,
	items: Vec<Item>,
	triggers: Vec<Item>,
}

impl Scheduler {
	/// The blocks describing the scheduler itself, in file order.
	pub fn properties(&self) -> &[Block] {
		&self.properties
	}

	/// The timelines the scheduler starts on its own.
	pub fn items(&self) -> &[Item] {
		&self.items
	}

	/// The timelines the scheduler starts when told to, which take the same shape as
	/// [`items`](Self::items).
	pub fn triggers(&self) -> &[Item] {
		&self.triggers
	}

	pub(super) fn parse(blocks: Vec<Block>) -> Self {
		let mut properties = Vec::new();
		let (mut item_list, mut trigger_list) = (None, None);
		for block in blocks {
			match block.name().as_str() {
				"Item" => item_list = Some(block),
				"Trgr" => trigger_list = Some(block),
				"ItCn" | "TrCn" => (),
				_ => properties.push(block),
			}
		}

		let started = items(item_list);
		let mut triggers = items(trigger_list);
		triggers.drain(..started.len().min(triggers.len()));

		Self {
			properties,
			items: started,
			triggers,
		}
	}
}

/// A span of frames and what runs over it.
///
/// Each entry carries `StTm` and `EdTm` bounding it, and `BdNo`, `EfNo` and `EmNo` indexing
/// [`Avfx::binders`], [`Avfx::effectors`] and [`Avfx::emitters`], any of which may be `-1`.
///
/// [`Avfx::binders`]: super::Avfx::binders
/// [`Avfx::effectors`]: super::Avfx::effectors
/// [`Avfx::emitters`]: super::Avfx::emitters
#[derive(Debug)]
pub struct Timeline {
	properties: Vec<Block>,
	items: Vec<Item>,
	clips: Vec<Clip>,
}

impl Timeline {
	/// The blocks describing the timeline itself, in file order. `LpSt` and `LpEd` bound the span
	/// it loops over.
	pub fn properties(&self) -> &[Block] {
		&self.properties
	}

	/// What runs over the timeline, in file order.
	pub fn items(&self) -> &[Item] {
		&self.items
	}

	/// The points the timeline can be told to jump to or stop at.
	pub fn clips(&self) -> &[Clip] {
		&self.clips
	}

	pub(super) fn parse(blocks: Vec<Block>) -> Self {
		let mut properties = Vec::new();
		let mut item_list = None;
		let mut clips = Vec::new();
		for block in blocks {
			match block.name().as_str() {
				"Item" => item_list = Some(block),
				"Clip" => clips.push(Clip::parse(block.bytes())),
				"TICn" | "CpCn" => (),
				_ => properties.push(block),
			}
		}

		Self {
			properties,
			items: items(item_list),
			clips,
		}
	}
}

/// A point in a timeline that does something other than run an item.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Clip {
	/// What the clip does.
	kind: ClipKind,

	/// Four integers whose meaning depends on the [kind](Self::kind).
	integers: [i32; 4],

	/// Four floats whose meaning depends on the [kind](Self::kind).
	floats: [f32; 4],
}

impl Clip {
	fn parse(bytes: &[u8]) -> Self {
		let word = |at: usize| -> [u8; 4] {
			bytes
				.get(at..at + 4)
				.and_then(|raw| raw.try_into().ok())
				.unwrap_or_default()
		};

		Self {
			kind: word(0).into(),
			integers: std::array::from_fn(|index| i32::from_le_bytes(word(4 + index * 4))),
			floats: std::array::from_fn(|index| f32::from_le_bytes(word(20 + index * 4))),
		}
	}
}

/// What a [`Clip`] does when the timeline reaches it.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
	Kill,
	Reset,
	End,
	FadeIn,
	UnlockLoopPoint,
	Trigger,
	RandomTrigger,
	/// A kind ironworks does not recognise; the inner value is the raw tag.
	Unknown([u8; 4]),
}

impl From<[u8; 4]> for ClipKind {
	fn from(raw: [u8; 4]) -> Self {
		let mut tag = raw;
		tag.reverse();
		match &tag {
			b"KILL" => Self::Kill,
			b"REST" => Self::Reset,
			b"END " => Self::End,
			b"FADI" => Self::FadeIn,
			b"ULLP" => Self::UnlockLoopPoint,
			b"TRG " => Self::Trigger,
			b"RTRG" => Self::RandomTrigger,
			_ => Self::Unknown(tag),
		}
	}
}

/// What spawns particles, and what those particles are.
///
/// Each entry of both lists carries `bEnb` and `TgtB`, the latter indexing
/// [`Avfx::particles`] or [`Avfx::emitters`] depending on the list it sits in.
///
/// [`Avfx::particles`]: super::Avfx::particles
/// [`Avfx::emitters`]: super::Avfx::emitters
#[derive(Debug)]
pub struct Emitter {
	properties: Vec<Block>,
	particles: Vec<Item>,
	emitters: Vec<Item>,
}

impl Emitter {
	/// The blocks describing the emitter itself, in file order. `Data` holds whatever the
	/// emitter's `EVT` kind adds, and the curves driving it sit beside it.
	pub fn properties(&self) -> &[Block] {
		&self.properties
	}

	/// The particles this emitter spawns.
	pub fn particles(&self) -> &[Item] {
		&self.particles
	}

	/// The emitters this emitter spawns, which take the same shape as
	/// [`particles`](Self::particles).
	pub fn emitters(&self) -> &[Item] {
		&self.emitters
	}

	pub(super) fn parse(blocks: Vec<Block>) -> Self {
		let mut properties = Vec::new();
		let (mut particle_list, mut emitter_list) = (None, None);
		for block in blocks {
			match block.name().as_str() {
				"ItPr" => particle_list = Some(block),
				"ItEm" => emitter_list = Some(block),
				"PrCn" | "EmCn" => (),
				_ => properties.push(block),
			}
		}

		let particles = items(particle_list);
		let mut emitters = items(emitter_list);
		emitters.drain(..particles.len().min(emitters.len()));

		Self {
			properties,
			particles,
			emitters,
		}
	}
}
