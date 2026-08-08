//! Structs and utilities for parsing .essb files.

use crate::{FileStream, error::Result};

use super::{File, envs::Environments};

/// The environment a part of a zone is heard through.
#[derive(Debug)]
pub struct SoundEnvironmentFile(Environments);

impl SoundEnvironmentFile {
	/// The environment set the file holds.
	pub fn environments(&self) -> &Environments {
		&self.0
	}
}

impl File for SoundEnvironmentFile {
	fn read(stream: impl FileStream) -> Result<Self> {
		Ok(Self(super::envs::read(stream, b"ESSB")?))
	}
}
