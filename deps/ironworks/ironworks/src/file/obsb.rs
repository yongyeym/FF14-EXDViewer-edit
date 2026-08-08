//! Structs and utilities for parsing .obsb files.

use crate::{FileStream, error::Result};

use super::{File, envs::Environments};

/// The object behaviour set a part of a zone runs.
#[derive(Debug)]
pub struct ObjectBehaviourFile(Environments);

impl ObjectBehaviourFile {
	/// The environment set the file holds.
	pub fn environments(&self) -> &Environments {
		&self.0
	}
}

impl File for ObjectBehaviourFile {
	fn read(stream: impl FileStream) -> Result<Self> {
		Ok(Self(super::envs::read(stream, b"OBSB")?))
	}
}
