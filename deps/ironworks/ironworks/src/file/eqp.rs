//! Structs and utilities for parsing .eqp files.

use binrw::BinRead;

use crate::{FileStream, error::Result};

use super::{File, block_table::BlockTable};

/// The default entry, used for every set whose block is omitted from the file.
const DEFAULT: u64 = 0x3fe00070603f00;

/// Flags controlling how a piece of equipment hides and reveals the rest of the character.
#[derive(Debug)]
pub struct EquipmentParameter(BlockTable);

impl EquipmentParameter {
	/// Get flags for the specified set ID.
	pub fn set(&self, id: u16) -> Set {
		Set(bitfield::Set::from_bytes(
			self.0.entry(id, DEFAULT).to_le_bytes(),
		))
	}
}

impl File for EquipmentParameter {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(Self(<BlockTable as BinRead>::read(&mut stream)?))
	}
}

/// Flags for a specific set, one group per equipment slot.
#[derive(Debug)]
pub struct Set(bitfield::Set);

impl Set {
	/// Flags for equipment worn in the body slot.
	pub fn body(&self) -> Body {
		Body(self.0.body())
	}

	/// Flags for equipment worn in the legs slot.
	pub fn legs(&self) -> Legs {
		Legs(self.0.legs())
	}

	/// Flags for equipment worn in the hands slot.
	pub fn hands(&self) -> Hands {
		Hands(self.0.hands())
	}

	/// Flags for equipment worn in the feet slot.
	pub fn feet(&self) -> Feet {
		Feet(self.0.feet())
	}

	/// Flags for equipment worn in the head slot.
	pub fn head(&self) -> Head {
		Head(self.0.head())
	}
}

macro_rules! slot {
	($(#[$doc:meta])* $name:ident { $($flag:ident,)* }) => {
		$(#[$doc])*
		#[derive(Debug)]
		pub struct $name(bitfield::$name);

		impl $name {
			$(
				#[doc = concat!("The ", stringify!($flag), " flag.")]
				pub fn $flag(&self) -> bool {
					self.0.$flag()
				}
			)*
		}
	};
}

slot!(
	/// Body slot flags.
	Body {
		enabled,
		hide_waist,
		hide_thighs,
		hide_gloves_small,
		hide_glove_cuffs,
		hide_gloves_medium,
		hide_gloves_large,
		hide_gorget,
		show_legs,
		show_hands,
		show_head,
		show_necklace,
		show_bracelets,
		show_tail,
		disable_breast_physics,
		uses_vfx_parameter,
	}
);

slot!(
	/// Legs slot flags.
	Legs {
		enabled,
		hide_knee_pads,
		hide_boots_small,
		hide_boots_medium,
		show_feet,
		show_tail,
	}
);

slot!(
	/// Hands slot flags.
	Hands {
		enabled,
		hide_elbow,
		hide_forearm,
		show_bracelets,
		show_ring_left,
		show_ring_right,
	}
);

slot!(
	/// Feet slot flags.
	Feet {
		enabled,
		hide_knee,
		hide_calf,
		hide_ankle,
	}
);

slot!(
	/// Head slot flags.
	Head {
		enabled,
		hide_scalp,
		hide_hair,
		show_hair_override,
		hide_neck,
		show_necklace,
		show_earrings_hyur_roegadyn,
		show_earrings_elezen_lalafell,
		show_earrings_miqote_hrothgar_viera,
		show_earrings_au_ra,
		show_ears_human,
		show_ears_miqote,
		show_ears_au_ra,
		show_ears_viera,
		disable_bangs_physics,
		disable_hair_physics,
		show_on_hrothgar,
		show_on_viera,
		uses_vfx_parameter,
	}
);

#[allow(dead_code)]
mod bitfield {
	use modular_bitfield::prelude::*;

	#[bitfield]
	#[derive(Debug)]
	pub struct Set {
		pub body: Body,
		pub legs: Legs,
		pub hands: Hands,
		pub feet: Feet,
		pub head: Head,
	}

	#[bitfield(bits = 16)]
	#[derive(BitfieldSpecifier, Debug)]
	pub struct Body {
		pub enabled: bool,
		pub hide_waist: bool,
		pub hide_thighs: bool,
		pub hide_gloves_small: bool,
		pub hide_glove_cuffs: bool,
		pub hide_gloves_medium: bool,
		pub hide_gloves_large: bool,
		pub hide_gorget: bool,
		pub show_legs: bool,
		pub show_hands: bool,
		pub show_head: bool,
		pub show_necklace: bool,
		pub show_bracelets: bool,
		pub show_tail: bool,
		pub disable_breast_physics: bool,
		pub uses_vfx_parameter: bool,
	}

	#[bitfield(bits = 8)]
	#[derive(BitfieldSpecifier, Debug)]
	pub struct Legs {
		pub enabled: bool,
		pub hide_knee_pads: bool,
		pub hide_boots_small: bool,
		pub hide_boots_medium: bool,
		#[skip]
		_unknown_20: bool,
		pub show_feet: bool,
		pub show_tail: bool,
		#[skip]
		_unknown_23: bool,
	}

	#[bitfield(bits = 8)]
	#[derive(BitfieldSpecifier, Debug)]
	pub struct Hands {
		pub enabled: bool,
		pub hide_elbow: bool,
		pub hide_forearm: bool,
		#[skip]
		_unknown_27: bool,
		pub show_bracelets: bool,
		pub show_ring_left: bool,
		pub show_ring_right: bool,
		#[skip]
		_unknown_31: bool,
	}

	#[bitfield(bits = 8)]
	#[derive(BitfieldSpecifier, Debug)]
	pub struct Feet {
		pub enabled: bool,
		pub hide_knee: bool,
		pub hide_calf: bool,
		pub hide_ankle: bool,
		#[skip]
		_unknown_36: B4,
	}

	#[bitfield(bits = 24)]
	#[derive(BitfieldSpecifier, Debug)]
	pub struct Head {
		pub enabled: bool,
		pub hide_scalp: bool,
		pub hide_hair: bool,
		pub show_hair_override: bool,
		pub hide_neck: bool,
		pub show_necklace: bool,
		pub show_earrings_hyur_roegadyn: bool,
		pub show_earrings_elezen_lalafell: bool,
		pub show_earrings_miqote_hrothgar_viera: bool,
		pub show_earrings_au_ra: bool,
		pub show_ears_human: bool,
		pub show_ears_miqote: bool,
		pub show_ears_au_ra: bool,
		pub show_ears_viera: bool,
		pub disable_bangs_physics: bool,
		pub disable_hair_physics: bool,
		pub show_on_hrothgar: bool,
		pub show_on_viera: bool,
		pub uses_vfx_parameter: bool,
		#[skip]
		_unknown_59: B5,
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::EquipmentParameter;

	/// Build a file from the blocks it carries, each padded out to its full 160 entries. `control`
	/// occupies entry 0 of the first block.
	fn parameters(control: u64, blocks: &[&[u64]]) -> Vec<u8> {
		let mut bytes = Vec::new();
		for block in blocks {
			let mut entries = block.to_vec();
			entries.resize(160, 0);
			bytes.extend(entries.iter().flat_map(|entry| entry.to_le_bytes()));
		}
		bytes[..8].copy_from_slice(&control.to_le_bytes());
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			EquipmentParameter::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn unpacks_flags_for_every_slot() {
		let entry = 1 | (1 << 15) | (1 << 16) | (1 << 24) | (1 << 32) | (1 << 40) | (1 << 58);
		let file = EquipmentParameter::read(Cursor::new(parameters(1, &[&[0, entry]]))).unwrap();

		let set = file.set(1);
		assert!(set.body().enabled());
		assert!(set.legs().enabled());
		assert!(set.hands().enabled());
		assert!(set.feet().enabled());
		assert!(set.head().enabled());
		assert!(set.body().uses_vfx_parameter());
		assert!(set.head().uses_vfx_parameter());
		assert!(!set.body().hide_waist());
	}

	/// Set 0 has no entry of its own - entry 0 is the control word - so the game reads set 1.
	#[test]
	fn set_zero_aliases_set_one() {
		let file = EquipmentParameter::read(Cursor::new(parameters(1, &[&[0, 1]]))).unwrap();

		assert!(file.set(0).body().enabled());
	}

	#[test]
	fn an_omitted_block_reads_as_the_default() {
		// Blocks 0 and 2, so the second stored block holds sets 320 onwards.
		let file =
			EquipmentParameter::read(Cursor::new(parameters(0b101, &[&[], &[1 << 40]]))).unwrap();

		assert!(file.set(320).head().enabled());

		// Set 160 falls in the omitted block, 480 past the last, 60000 past the last possible.
		for id in [160, 480, 60000] {
			let set = file.set(id);
			assert!(!set.head().enabled());
			assert!(set.body().show_legs());
			assert!(set.hands().show_ring_left());
			assert!(set.head().show_ears_human());
		}
	}

	/// A block the control word declares but the file cuts short reads as the default too.
	#[test]
	fn a_block_truncated_by_the_file_reads_as_the_default() {
		let mut bytes = parameters(1, &[&[0, 1]]);
		bytes.truncate(40);
		let file = EquipmentParameter::read(Cursor::new(bytes)).unwrap();

		assert!(file.set(1).body().enabled());
		assert!(!file.set(100).body().enabled());
		assert!(file.set(100).body().show_legs());
	}
}
