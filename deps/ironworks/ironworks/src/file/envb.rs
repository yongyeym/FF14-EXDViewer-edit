//! Structs and utilities for parsing .envb files.

use crate::{FileStream, error::Result};

use super::{File, envs::Environments};

/// The environment a part of a zone is lit and shaded with.
#[derive(Debug)]
pub struct EnvironmentFile(Environments);

impl EnvironmentFile {
	/// The environment set the file holds.
	pub fn environments(&self) -> &Environments {
		&self.0
	}
}

impl File for EnvironmentFile {
	fn read(stream: impl FileStream) -> Result<Self> {
		Ok(Self(super::envs::read(stream, b"ENVB")?))
	}
}
