/// Standard CRC-32 without the final XOR.
pub fn crc32(bytes: &[u8]) -> u32 {
	!crc32fast::hash(bytes)
}
