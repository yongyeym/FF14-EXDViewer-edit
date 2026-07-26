//! Structs and utilities for parsing .scd files.

mod container;
mod entry;

pub use {
	container::SoundContainer,
	entry::{Codec, SoundEntry},
};
