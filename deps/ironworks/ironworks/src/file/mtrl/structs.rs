use binrw::binread;

/// On-disk layout of a material.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Material {
	// Container header
	pub version: u32,
	_file_size: u16,
	#[br(temp)]
	data_set_size: u16,
	#[br(temp)]
	string_table_size: u16,
	pub shader_package_name_offset: u16,
	#[br(temp)]
	texture_count: u8,
	#[br(temp)]
	uv_set_count: u8,
	#[br(temp)]
	color_set_count: u8,
	#[br(temp)]
	additional_data_size: u8,

	#[br(count = texture_count)]
	pub texture_offsets: Vec<TextureOffset>,

	#[br(count = uv_set_count)]
	pub uv_sets: Vec<AttributeSet>,

	#[br(count = color_set_count)]
	pub color_sets: Vec<AttributeSet>,

	#[br(count = string_table_size)]
	pub string_data: Vec<u8>,

	/// Trailing bytes of the container header. The first four are a flag word describing the colour
	/// table; the remainder is unidentified.
	#[br(count = additional_data_size)]
	pub additional_data: Vec<u8>,

	/// The colour table, when the material carries one. Kept as raw halves because the row layout
	/// differs between the legacy and current forms, which only the total size distinguishes.
	#[br(if(data_set_size > 0), count = usize::from(data_set_size) / 2)]
	pub color_table: Option<Vec<u16>>,

	// Material header
	#[br(temp)]
	shader_value_list_size: u16,
	#[br(temp)]
	shader_key_count: u16,
	#[br(temp)]
	constant_count: u16,
	#[br(temp)]
	sampler_count: u16,
	pub shader_flags: u32,

	#[br(count = shader_key_count)]
	pub shader_keys: Vec<ShaderKey>,

	#[br(count = constant_count)]
	pub constants: Vec<Constant>,

	#[br(count = sampler_count)]
	pub samplers: Vec<Sampler>,

	#[br(count = shader_value_list_size / 4)]
	pub shader_values: Vec<f32>,
}

#[binread]
#[br(little)]
#[derive(Debug)]
pub struct TextureOffset {
	pub offset: u16,
	pub flags: u16,
}

/// A named index, used for both UV sets and colour sets.
#[binread]
#[br(little)]
#[derive(Debug)]
pub struct AttributeSet {
	pub name_offset: u16,
	pub index: u16,
}

#[binread]
#[br(little)]
#[derive(Debug)]
pub struct ShaderKey {
	pub category: u32,
	pub value: u32,
}

#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Constant {
	pub id: u32,
	/// Byte offset into the shader value list, and the byte length taken from it. Sizes are a
	/// multiple of four; 12 is a vec3.
	pub value_offset: u16,
	pub value_size: u16,
}

#[binread]
#[br(little)]
#[derive(Debug)]
pub struct Sampler {
	pub id: u32,
	pub flags: u32,
	#[br(pad_after = 3)]
	pub texture_index: u8,
	// padding: [u8; 3].
}
