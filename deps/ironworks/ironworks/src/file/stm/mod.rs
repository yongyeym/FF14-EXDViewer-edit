//! Structs and utilities for parsing .stm files.
//!
//! Two of these ship. `chara/base_material/stainingtemplate.stm` holds templates 100 to 612 and
//! `chara/base_material/stainingtemplate_gud.stm` holds 1100 to 1612, so a dye row's template id
//! says which of the two to read.

mod structs;
mod templates;

pub use templates::{DyePack, StainingTemplates, Template};

use crate::error::{Error, ErrorValue};

fn invalid(reason: impl Into<String>) -> Error {
	Error::Invalid(ErrorValue::Other("STM".into()), reason.into())
}
