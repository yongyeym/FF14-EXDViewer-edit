//! Structs and utilities for parsing .uld files.

mod component;
mod layout;
mod node;
mod structs;
mod timeline;

pub use {
	component::{Component, ComponentKind},
	layout::{Alignment, Part, PartList, Texture, UiLayout, Widget},
	node::{
		ClippingMask, Collision, ComponentInstance, Counter, Font, Image, NineGrid, Node,
		NodeFlags, NodeKind, Text, TextFlags,
	},
	structs::Version,
	timeline::{Animation, KeyGroup, KeyGroupKind, KeyUsage, Timeline},
};

use std::io::Cursor;

use crate::error::{Error, ErrorValue, Result};

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("ULD".into()), reason.into())
}

/// A cursor over the file from `at` onwards.
fn cursor(bytes: &[u8], at: usize) -> Result<Cursor<&[u8]>> {
	match bytes.get(at..) {
		Some(rest) => Ok(Cursor::new(rest)),
		None => Err(invalid(format!(
			"offset {at:#x} is past the end of the file"
		))),
	}
}

/// The payload of a record that starts at `at` and declares itself `size` bytes long, being
/// everything after its `prefix`-byte header.
fn region(bytes: &[u8], at: usize, size: usize, prefix: usize) -> Result<&[u8]> {
	match at.checked_add(size) {
		Some(end) if size >= prefix && end <= bytes.len() => Ok(&bytes[at + prefix..end]),
		_ => Err(invalid(format!(
			"record at {at:#x} declares an unusable size of {size}"
		))),
	}
}
