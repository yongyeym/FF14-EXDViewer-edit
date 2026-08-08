use getset::{CopyGetters, Getters};

use crate::error::Result;

use super::{LayerGroup, groups, i32_at, invalid, seek, string};

/// Fields the scene header holds, each an offset from the header's own body.
const FIELDS: usize = 16;

/// One entry of the environment list the general fields point at.
const ENVIRONMENT: usize = 24;

/// One entry of the filter list.
const FILTER: usize = 28;

/// An environment the scene applies over part of itself.
#[derive(Debug, Getters, CopyGetters)]
pub struct Environment {
	/// The `.envb` the environment is described by.
	#[get = "pub"]
	asset_path: String,

	#[get_copy = "pub"]
	index: i32,

	/// The [`EnvLocation`](super::EnvLocation) instance the environment is centred on.
	#[get_copy = "pub"]
	env_location_instance_id: i32,

	/// The `.essb` the environment is heard through.
	#[get = "pub"]
	sound_asset_path: String,
}

/// A place the scene is used from. Several territories can share one scene, and each names the
/// layers it turns on through one of these.
#[derive(Debug, Clone, Copy, CopyGetters)]
#[get_copy = "pub"]
pub struct Filter {
	key: u32,

	/// A row of `TerritoryType`.
	territory_type: u16,

	/// A row of `ContentFinderCondition`, and zero where the scene is not entered through one.
	content_finder_condition: u16,
}

/// Everything an `SCN1` section holds: the layer groups laid out inside the file, the paths of the
/// ones kept beside it, and what the scene is drawn and heard with.
#[derive(Debug, Getters)]
#[get = "pub"]
pub struct Scene {
	#[getset(skip)]
	layer_groups: Vec<LayerGroup>,

	/// The `.lgb` files the scene draws its remaining layer groups from.
	layer_group_paths: Vec<String>,

	/// The directory the scene's own assets sit under.
	bg_path: String,

	/// The `.svb` saying which of the scene's models the sky reaches.
	sky_visibility_path: String,

	/// The `.lcb` bounding the scene's lights.
	light_culling_path: String,

	#[getset(skip)]
	environments: Vec<Environment>,

	#[getset(skip)]
	filters: Vec<Filter>,
}

impl Scene {
	/// The layer groups written into the file itself, in the order the header names them.
	pub fn layer_groups(&self) -> &[LayerGroup] {
		&self.layer_groups
	}

	/// The environments the scene applies, in the order it names them.
	pub fn environments(&self) -> &[Environment] {
		&self.environments
	}

	/// The places the scene is used from, in the order it names them.
	pub fn filters(&self) -> &[Filter] {
		&self.filters
	}

	/// Reads the scene whose section header starts at `at`.
	pub(super) fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		// The older section puts two empty fields ahead of the body.
		let body = match (i32_at(bytes, at + 8)?, i32_at(bytes, at + 12)?) {
			(0, 0) => at + 16,
			_ => at + 8,
		};

		let offsets = (0..FIELDS)
			.map(|slot| i32_at(bytes, body + slot * 4))
			.collect::<Result<Vec<_>>>()?;
		// Rejected where the bytes behind the table cannot hold that many: collecting a range
		// reserves the whole count before the first read fails.
		let count = |declared: i32, table: usize, stride: usize| {
			let count = usize::try_from(declared)
				.map_err(|_| invalid(format!("a scene at {at:#x} declaring {declared} entries")))?;
			let room = bytes.len().saturating_sub(table) / stride;
			match count <= room {
				true => Ok(count),
				false => Err(invalid(format!(
					"a scene at {at:#x} declaring {count} entries in room for {room}"
				))),
			}
		};

		let heaps = (0..count(offsets[1], seek(body, offsets[0])?, 16)?)
			.map(|index| Ok(seek(body, offsets[0])? + index * 16))
			.collect::<Result<Vec<_>>>()?;

		let table = seek(body, offsets[5])?;
		let layer_group_paths = (0..count(offsets[6], table, size_of::<i32>())?)
			.map(|index| {
				Ok(string(
					bytes,
					seek(table, i32_at(bytes, table + index * 4)?)?,
				))
			})
			.collect::<Result<Vec<_>>>()?;

		let list = seek(body, offsets[3])?;
		let entries = seek(list, i32_at(bytes, list)?)?;
		let filters = (0..count(i32_at(bytes, list + 4)?, entries, FILTER)?)
			.map(|index| Filter::parse(bytes, entries + index * FILTER))
			.collect::<Result<Vec<_>>>()?;

		let general = seek(body, offsets[2])?;
		let path = |offset| -> Result<String> {
			Ok(string(
				bytes,
				seek(general, i32_at(bytes, general + offset)?)?,
			))
		};
		let list = seek(general, i32_at(bytes, general + 8)?)?;
		let environments = (0..count(i32_at(bytes, general + 12)?, list, ENVIRONMENT)?)
			.map(|index| Environment::parse(bytes, list + index * ENVIRONMENT))
			.collect::<Result<Vec<_>>>()?;

		// Whatever the header lays out after a layer group bounds that group's last instance.
		let mut rest = heaps.clone();
		for &offset in &offsets {
			if offset > 0 {
				rest.push(seek(body, offset)?);
			}
		}

		Ok(Self {
			layer_groups: groups(bytes, &heaps, &rest)?,
			layer_group_paths,
			bg_path: path(4)?,
			sky_visibility_path: path(20)?,
			light_culling_path: path(52)?,
			environments,
			filters,
		})
	}
}

impl Filter {
	fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		let ids = i32_at(bytes, at + 16)? as u32;
		Ok(Self {
			key: i32_at(bytes, at + 4)? as u32,
			territory_type: ids as u16,
			content_finder_condition: (ids >> 16) as u16,
		})
	}
}

impl Environment {
	fn parse(bytes: &[u8], at: usize) -> Result<Self> {
		Ok(Self {
			asset_path: string(bytes, seek(at, i32_at(bytes, at)?)?),
			index: i32_at(bytes, at + 4)?,
			env_location_instance_id: i32_at(bytes, at + 8)?,
			sound_asset_path: string(bytes, seek(at, i32_at(bytes, at + 12)?)?),
		})
	}
}
