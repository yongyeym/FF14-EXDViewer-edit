//! Structs and utilities for parsing .lvb files.

use crate::{FileStream, error::Result};

use super::{File, layer::Scene};

/// A zone's level: the scene naming the layer groups and settings the zone is built from.
#[derive(Debug)]
pub struct LevelFile(Scene);

impl LevelFile {
	/// The scene the file holds.
	pub fn scene(&self) -> &Scene {
		&self.0
	}
}

impl File for LevelFile {
	fn read(stream: impl FileStream) -> Result<Self> {
		Ok(Self(super::layer::scene(stream, b"LVB1")?))
	}
}
