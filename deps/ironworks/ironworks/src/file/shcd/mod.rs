//! Structs and utilities for parsing .shcd files.

mod code;
mod structs;

pub use {
	crate::file::shader::{DirectX, Resource},
	code::{ShaderCode, Stage},
};
