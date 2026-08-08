//! Structs and utilities for parsing .pbd files.

use std::{
	fmt,
	io::{Read, Seek, SeekFrom},
};

use binrw::{BinRead, BinResult, Endian, NullString, binread};

use crate::{FileStream, error::Result};

use super::file::File;

/// Collection of bone deformations for transforming between character skeletons.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct PreBoneDeformer {
	#[br(temp)]
	data_count: u32,

	#[br(count = data_count)]
	deformers: Vec<DeformerData>,

	#[br(count = data_count)]
	nodes: Vec<NodeData>,
}

impl PreBoneDeformer {
	/// Get an iterator over the deformers in this file.
	pub fn deformers(&self) -> impl Iterator<Item = Deformer<'_>> {
		self.deformers.iter().map(|deformer| Deformer {
			pbd: self,
			deformer,
		})
	}

	/// Get the root of the node tree.
	pub fn root_node(&self) -> Option<Node<'_>> {
		self.nodes
			.iter()
			.find(|node| node.parent_index == u16::MAX)
			.map(|node| Node { pbd: self, node })
	}
}

impl File for PreBoneDeformer {
	fn read(mut stream: impl FileStream) -> Result<Self> {
		Ok(<Self as BinRead>::read(&mut stream)?)
	}
}

/// A node within the deformer tree.
pub struct Node<'a> {
	pbd: &'a PreBoneDeformer,
	node: &'a NodeData,
}

impl Node<'_> {
	/// Get this node's corresponding deformer.
	pub fn deformer(&self) -> Deformer<'_> {
		Deformer {
			pbd: self.pbd,
			deformer: &self.pbd.deformers[usize::from(self.node.deformer_index)],
		}
	}

	/// Get the parent node within the tree.
	pub fn parent(&self) -> Option<Node<'_>> {
		self.get_relation(self.node.parent_index)
	}

	/// Get the first child node, if this node has any children.
	pub fn first_child(&self) -> Option<Node<'_>> {
		self.get_relation(self.node.first_child_index)
	}

	/// Get the next sibling node.
	pub fn next(&self) -> Option<Node<'_>> {
		self.get_relation(self.node.next_index)
	}

	fn get_relation(&self, index: u16) -> Option<Node<'_>> {
		match index {
			u16::MAX => None,
			index => Some(Node {
				pbd: self.pbd,
				node: &self.pbd.nodes[usize::from(index)],
			}),
		}
	}
}

impl fmt::Debug for Node<'_> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.node.fmt(formatter)
	}
}

/// Deformer information for a character ID.
pub struct Deformer<'a> {
	pbd: &'a PreBoneDeformer,
	deformer: &'a DeformerData,
}

impl Deformer<'_> {
	/// Get this deformer's corresponding node in the tree.
	pub fn node(&self) -> Node<'_> {
		Node {
			pbd: self.pbd,
			node: &self.pbd.nodes[usize::from(self.deformer.node_index)],
		}
	}

	/// Get the character ID this deformer represents.
	pub fn id(&self) -> u16 {
		self.deformer.id
	}

	/// Get the bones this deformer moves, in the order it names them, if it has any.
	pub fn bones(&self) -> Option<&[(String, BoneMatrix)]> {
		self.deformer
			.bone_matrices
			.as_ref()
			.map(|matrices| &matrices.bones[..])
	}

	/// Get the scale this deformer is applied at.
	pub fn scale(&self) -> f32 {
		self.deformer.scale
	}
}

impl fmt::Debug for Deformer<'_> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.deformer.fmt(formatter)
	}
}

#[binread]
#[br(little)]
#[derive(Debug)]
struct DeformerData {
	id: u16,
	node_index: u16,

	#[br(temp)]
	data_offset: i32,

	#[br(
		if(data_offset > 0),
		seek_before = SeekFrom::Start(data_offset.try_into().unwrap()),
		restore_position,
	)]
	bone_matrices: Option<BoneMatrices>,

	// TODO: apparently 2.x pbds don't include this?
	scale: f32,
}

/// The rows of a bone's transform, the fourth left off.
pub type BoneMatrix = [[f32; 4]; 3];

#[binread]
#[br(little)]
struct BoneMatrices {
	#[br(temp, parse_with = current_position)]
	base_offset: u64,

	#[br(temp)]
	bone_count: u32,

	#[br(temp, args {
		count: bone_count.try_into().unwrap(),
		inner: (base_offset,)
	})]
	bone_names: Vec<BoneName>,

	#[br(temp, align_before = 4, count = bone_count)]
	matrices: Vec<BoneMatrix>,

	#[br(calc = (0..bone_count)
		.map(|index| {
			let i = usize::try_from(index).unwrap();
			(bone_names[i].bone_name.to_string(), matrices[i])
		})
		.collect()
	)]
	bones: Vec<(String, BoneMatrix)>,
}

impl fmt::Debug for BoneMatrices {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.bones.fmt(formatter)
	}
}

#[binread]
#[br(little, import(base_offset: u64))]
#[derive(Debug)]
struct BoneName {
	#[br(temp)]
	offset: i16,

	#[br(
		seek_before = SeekFrom::Start(base_offset + u64::try_from(offset).unwrap()),
		restore_position,
	)]
	bone_name: NullString,
}

#[binread]
#[br(little)]
#[derive(Debug)]
struct NodeData {
	parent_index: u16,
	first_child_index: u16,
	next_index: u16,
	deformer_index: u16,
}

fn current_position<R: Read + Seek>(reader: &mut R, _: Endian, _: ()) -> BinResult<u64> {
	Ok(reader.stream_position()?)
}
