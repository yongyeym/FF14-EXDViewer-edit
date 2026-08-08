//! Structs and utilities for parsing .gfd files.

use binrw::{BinRead, binread};
use getset::CopyGetters;

use crate::{FileStream, error::Result};

use super::File;

/// The icons text can be drawn with, as rectangles of the gamepad button sheets in
/// `common/font/fontIcon_*.tex`.
///
/// Every sheet holds the same icons in the same places, drawn for a different controller, so an
/// entry names a rectangle rather than a file.
#[binread]
#[br(little, magic = b"gftd0100")]
#[derive(Debug)]
pub struct FontIcons {
	#[br(temp, pad_after = 4)]
	count: u32,

	#[br(count = count)]
	icons: Vec<Icon>,
}

impl FontIcons {
	/// Every icon, ordered by id.
	pub fn icons(&self) -> &[Icon] {
		&self.icons
	}

	/// The icon an id names, following the redirect where one icon is drawn as another. `None` for
	/// an id the file does not carry, and for one carrying no rectangle.
	///
	/// An icon drawn as itself redirects to nothing, which ends the walk; only a loop can run on,
	/// and taking no more steps than there are icons bounds one.
	pub fn icon(&self, id: u16) -> Option<&Icon> {
		std::iter::successors(self.find(id), |icon| self.find(icon.redirect))
			.take(self.icons.len())
			.last()
			.filter(|icon| icon.redirect == 0 && icon.width > 0 && icon.height > 0)
	}

	/// Ids run from one without a gap in every file the game ships, so the id is its own index.
	fn find(&self, id: u16) -> Option<&Icon> {
		let at = usize::from(id.checked_sub(1)?);
		self.icons.get(at).filter(|icon| icon.id == id).or_else(|| {
			let index = self.icons.binary_search_by_key(&id, Icon::id).ok()?;
			self.icons.get(index)
		})
	}
}

/// One icon, as a rectangle of the sheet.
///
/// The sheet holds each icon twice: once at the stated rectangle, and once at twice the size for
/// text drawn large enough to need it.
#[binread]
#[br(little)]
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Icon {
	id: u16,

	/// Position within the sheet, in pixels.
	left: u16,
	top: u16,

	width: u16,
	height: u16,

	unknown_a: u16,

	/// The icon drawn in this one's place, or zero where it is drawn as itself.
	redirect: u16,

	unknown_e: u16,
}

impl Icon {
	/// Where the same icon is drawn at twice the size, in pixels.
	pub fn large(&self) -> (u16, u16, u16, u16) {
		const BELOW: u16 = 341;
		(
			self.left * 2,
			self.top * 2 + BELOW,
			self.width * 2,
			self.height * 2,
		)
	}
}

impl File for FontIcons {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

#[cfg(test)]
mod test {
	use std::io::{self, Cursor};

	use crate::{error::Error, file::File};

	use super::FontIcons;

	fn icons(entries: &[(u16, u16, u16, u16, u16, u16)]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(b"gftd0100");
		bytes.extend(u32::try_from(entries.len()).unwrap().to_le_bytes());
		bytes.extend([0; 4]);
		for &(id, left, top, width, height, redirect) in entries {
			for field in [id, left, top, width, height, 0, redirect, 0] {
				bytes.extend(field.to_le_bytes());
			}
		}
		bytes
	}

	#[test]
	fn empty() {
		assert!(matches!(
			FontIcons::read(io::empty()),
			Err(Error::Resource(_))
		));
	}

	#[test]
	fn reads_rectangles() {
		let file = FontIcons::read(Cursor::new(icons(&[
			(1, 0, 0, 20, 20, 0),
			(2, 20, 0, 20, 20, 0),
		])))
		.unwrap();
		assert_eq!(file.icons().len(), 2);

		let icon = file.icon(2).unwrap();
		assert_eq!((icon.left(), icon.top()), (20, 0));
		assert_eq!((icon.width(), icon.height()), (20, 20));
		assert_eq!(icon.large(), (40, 341, 40, 40));
		assert!(file.icon(3).is_none());
	}

	#[test]
	fn follows_a_redirect_to_the_icon_actually_drawn() {
		let file = FontIcons::read(Cursor::new(icons(&[
			(1, 0, 0, 20, 20, 0),
			(2, 0, 0, 0, 0, 1),
		])))
		.unwrap();
		assert_eq!(file.icon(2).unwrap().id(), 1);
	}

	/// An icon pointing at itself, or at another that points back, must not spin.
	#[test]
	fn a_redirect_loop_terminates() {
		let file = FontIcons::read(Cursor::new(icons(&[
			(1, 0, 0, 0, 0, 2),
			(2, 0, 0, 0, 0, 1),
		])))
		.unwrap();
		assert!(file.icon(1).is_none());
	}
}
