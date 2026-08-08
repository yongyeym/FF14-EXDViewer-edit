//! Structs and utilities for parsing .mdl files.

mod container;
mod mesh;
mod model;
mod structs;

pub use {
	container::ModelContainer,
	mesh::{Mesh, Submesh, VertexAttribute, VertexValues},
	model::{Lod, MeshKind, Model, Shape},
	structs::{VertexAttributeKind, VertexFormat},
};
