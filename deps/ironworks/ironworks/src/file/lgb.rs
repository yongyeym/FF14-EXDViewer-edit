//! Structs and utilities for parsing .lgb files.

use crate::{FileStream, error::Result};

use super::{File, layer::LayerGroup};

/// A zone's layer group: where everything in a zone is placed, one file per group.
///
/// A zone ships seven of these beside its models, named for the group each carries: `bg`,
/// `planmap`, `planner`, `planevent`, `planlive`, `sound` and `vfx`.
#[derive(Debug)]
pub struct LayerGroupFile(LayerGroup);

impl LayerGroupFile {
	/// The group the file holds.
	pub fn group(&self) -> &LayerGroup {
		&self.0
	}
}

impl File for LayerGroupFile {
	fn read(stream: impl FileStream) -> Result<Self> {
		let (bytes, at) = super::layer::section(stream, b"LGB1", b"LGP1")?;

		// A section states its own header length, and the older files put eight more bytes in it.
		let size = super::layer::section_size(&bytes, at)?;
		Ok(Self(super::layer::section_group(&bytes, at, size)?))
	}
}

#[cfg(test)]
mod test {
	use std::io::Cursor;

	use crate::file::{
		File,
		layer::{
			AnimationState, DoorState, InstanceData, InstanceKind, MovePathMode, PopKind,
			RotationKind, RotationState, SoundEffectKind,
		},
	};

	use super::LayerGroupFile;

	/// Builds a file around one layer holding `instances`, each already laid out.
	///
	/// `section` is the section header's declared length, which is what everything inside it is
	/// measured from; the older files write 32 where the rest write 24.
	fn build(section: usize, instances: &[Vec<u8>]) -> Vec<u8> {
		const LAYER_HEADER: usize = 52;

		let mut bytes = Vec::from(*b"LGB1");
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(1u32.to_le_bytes());

		let at = bytes.len();
		bytes.extend(*b"LGP1");
		bytes.extend((section as u32).to_le_bytes());
		bytes.resize(at + section - 16, 0);

		// Offsets inside the section are measured from the four fields that end its header.
		let heap = bytes.len();
		let table = heap + 16;
		let layer = table + 4;
		let instance_table = layer + LAYER_HEADER;
		let first = instance_table + instances.len() * 4;

		let names = first + instances.iter().map(Vec::len).sum::<usize>();
		bytes.extend(256u32.to_le_bytes());
		bytes.extend(((names - heap) as i32).to_le_bytes());
		bytes.extend(16u32.to_le_bytes());
		bytes.extend(1u32.to_le_bytes());
		bytes.extend(((layer - table) as i32).to_le_bytes());

		bytes.extend(7u32.to_le_bytes());
		bytes.extend(((names + 6 - layer) as i32).to_le_bytes());
		bytes.extend((LAYER_HEADER as u32).to_le_bytes());
		bytes.extend((instances.len() as u32).to_le_bytes());
		bytes.extend([1, 0, 0, 1]);
		bytes.extend(0u32.to_le_bytes());
		bytes.extend(3u16.to_le_bytes());
		bytes.extend(4u16.to_le_bytes());
		bytes.resize(layer + LAYER_HEADER, 0);

		let mut offset = first;
		for instance in instances {
			bytes.extend(((offset - instance_table) as i32).to_le_bytes());
			offset += instance.len();
		}
		for instance in instances {
			bytes.extend(instance);
		}
		bytes.extend(b"group\0");
		bytes.extend(b"layer\0");
		bytes
	}

	/// The 0x30 bytes every instance opens with, then whatever its kind adds.
	fn instance(kind: i32, id: u32, payload: &[u8]) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(kind.to_le_bytes());
		bytes.extend(id.to_le_bytes());
		bytes.extend(0i32.to_le_bytes());
		for value in [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0] {
			bytes.extend(value.to_le_bytes());
		}
		bytes.extend(payload);
		bytes
	}

	fn read(bytes: Vec<u8>) -> LayerGroupFile {
		LayerGroupFile::read(Cursor::new(bytes)).unwrap()
	}

	#[test]
	fn reads_a_layer_and_the_instance_on_it() {
		let file = read(build(24, &[instance(1, 42, &[0; 44])]));
		let group = file.group();
		assert_eq!(group.id(), 256);
		assert_eq!(group.name(), "group");

		let [layer] = group.layers() else {
			panic!("expected one layer")
		};
		assert_eq!(layer.name(), "layer");
		assert_eq!((layer.festival_id(), layer.festival_phase_id()), (3, 4));
		assert!(layer.visible());

		let [placed] = layer.instances() else {
			panic!("expected one instance")
		};
		assert_eq!(placed.kind(), InstanceKind::BgPart);
		assert_eq!(placed.id(), 42);
		assert_eq!(placed.transform().translation(), [1.0, 2.0, 3.0]);
		assert!(matches!(placed.data(), InstanceData::BgPart(_)));
	}

	/// Nothing states an instance's length, so an unmodelled kind is bounded by whatever follows it.
	/// Physis has no such fallback and fails the whole file on a kind it does not model.
	#[test]
	fn an_unmodelled_kind_keeps_its_bytes() {
		let file = read(build(
			24,
			&[instance(9, 1, &[0xAA; 12]), instance(1, 2, &[0; 44])],
		));
		let [npc, _] = file.group().layers()[0].instances() else {
			panic!("expected two instances")
		};

		assert_eq!(npc.kind(), InstanceKind::BattleNpc);
		let InstanceData::Unknown(data) = npc.data() else {
			panic!("BattleNpc has no modelled payload, so its bytes should be kept")
		};
		assert_eq!(data, &[0xAA; 12]);
	}

	/// A shared group states where its overrides and its move path sit, both measured from the
	/// instance. The builder writes the group name straight after the instances, so an offset of
	/// the instance's own length lands on it.
	#[test]
	fn reads_a_shared_group_and_the_path_it_moves_along() {
		let mut payload = Vec::new();
		payload.extend(176i32.to_le_bytes());
		payload.extend(3i32.to_le_bytes());
		payload.extend([92i32, 1, 2].map(i32::to_le_bytes).concat());
		payload.extend([1u8, 0, 1, 0]);
		payload.extend(7u32.to_le_bytes());
		payload.extend(116i32.to_le_bytes());
		payload.extend([0i32, 0, 2].map(i32::to_le_bytes).concat());
		payload.extend([0xAAu8; 24]);

		payload.extend(2i32.to_le_bytes());
		payload.extend([1u8, 0]);
		payload.extend(30u16.to_le_bytes());
		payload.extend([1u8, 0, 0, 0]);
		payload.extend(2i32.to_le_bytes());
		payload.extend(11u16.to_le_bytes());
		payload.extend(12u16.to_le_bytes());
		payload.extend([4.0f32; 10].map(f32::to_le_bytes).concat());

		let file = read(build(24, &[instance(6, 1, &payload)]));
		let InstanceData::SharedGroup(shared) = file.group().layers()[0].instances()[0].data()
		else {
			panic!("expected a shared group")
		};
		assert_eq!(shared.asset_path(), "group");
		assert_eq!(shared.initial_door_state(), DoorState::Closed);
		assert_eq!(shared.initial_rotation_state(), RotationState::Stopped);
		assert_eq!(shared.initial_transform_state(), AnimationState::Play);
		assert_eq!(shared.initial_colour_state(), AnimationState::Replay);
		assert!(shared.random_timeline_auto_play());
		assert!(!shared.random_timeline_loop_playback());
		assert_eq!(shared.bound_client_path_instance_id(), 7);
		assert_eq!(shared.overrides(), &[0xAA; 24]);
		assert_eq!(shared.move_path().mode(), MovePathMode::Timeline);
		assert_eq!(shared.move_path().time(), 30);
		assert_eq!(shared.move_path().rotation(), RotationKind::YAxisOnly);
		assert_eq!(shared.move_path().swing_rotation_speed_range(), [4.0, 4.0]);
	}

	/// A sound's parameters sit where the payload says, and the geometry it covers after those.
	#[test]
	fn reads_a_sound_and_the_geometry_it_covers() {
		let mut payload = Vec::new();
		payload.extend(56i32.to_le_bytes());
		payload.extend(0i32.to_le_bytes());
		payload.extend(5i32.to_le_bytes());
		payload.extend([1u8, 1, 0, 0]);
		payload.extend(20i32.to_le_bytes());
		payload.extend(8i32.to_le_bytes());
		payload.extend(3u32.to_le_bytes());
		payload.extend([0xBBu8; 8]);

		let file = read(build(24, &[instance(7, 1, &payload)]));
		let InstanceData::Sound(sound) = file.group().layers()[0].instances()[0].data() else {
			panic!("expected a sound")
		};
		assert_eq!(sound.kind(), SoundEffectKind::Line);
		assert!(sound.auto_play() && sound.no_far_clip());
		assert_eq!(sound.point_selection(), 3);
		assert_eq!(sound.binary(), &[0xBB; 8]);
	}

	/// A pop range measures its positions from the offset field rather than from the instance.
	#[test]
	fn reads_a_pop_range_and_the_positions_around_it() {
		let mut payload = Vec::new();
		payload.extend(1i32.to_le_bytes());
		payload.extend(12i32.to_le_bytes());
		payload.extend(2i32.to_le_bytes());
		payload.extend(0.5f32.to_le_bytes());
		payload.extend(
			[1.0f32, 0.0, 2.0, 3.0, 0.0, 4.0]
				.map(f32::to_le_bytes)
				.concat(),
		);

		let file = read(build(24, &[instance(40, 1, &payload)]));
		let InstanceData::PopRange(pop) = file.group().layers()[0].instances()[0].data() else {
			panic!("expected a pop range")
		};
		assert_eq!(pop.kind(), PopKind::Player);
		assert_eq!(pop.inner_radius_ratio(), 0.5);
		assert_eq!(pop.positions(), &[[1.0, 0.0, 2.0], [3.0, 0.0, 4.0]]);
	}

	/// Older zone files declare a 32-byte section header rather than 24.
	#[test]
	fn a_longer_section_header_still_finds_its_fields() {
		let file = read(build(32, &[instance(1, 42, &[0; 44])]));
		assert_eq!(file.group().name(), "group");
		assert_eq!(file.group().layers()[0].instances()[0].id(), 42);
	}
}
