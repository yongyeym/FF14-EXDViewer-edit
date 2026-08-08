//! Structs and utilities for parsing .shpk files.

mod package;
mod structs;

pub use {
	crate::file::shader::{DirectX, Resource},
	package::{
		AliasCluster, Key, MaterialParam, NONE, Node, NodeAlias, Pass, Shader, ShaderPackage,
		Stage, SubCluster,
	},
};
