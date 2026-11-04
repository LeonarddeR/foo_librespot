//! The Windows `GUID` that identifies foobar2000 service interfaces.

/// A Windows `GUID`, laid out exactly as the C struct.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Guid {
	pub data1: u32,
	pub data2: u16,
	pub data3: u16,
	pub data4: [u8; 8],
}

impl Guid {
	/// The all-zero GUID, `pfc::guid_null`.
	pub const NULL: Self = Self::from_u128(0);

	/// Builds a GUID from its usual written form, so `{436547fc-c4ef-4322-b59e-e696a25fab2c}`
	/// becomes `Guid::from_u128(0x436547fc_c4ef_4322_b59e_e696a25fab2c)`.
	#[allow(clippy::cast_possible_truncation)]
	pub const fn from_u128(value: u128) -> Self {
		Self {
			data1: (value >> 96) as u32,
			data2: (value >> 80) as u16,
			data3: (value >> 64) as u16,
			data4: (value as u64).to_be_bytes(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::Guid;

	#[test]
	fn guid_from_u128_matches_fields() {
		let guid = Guid::from_u128(0x436547fc_c4ef_4322_b59e_e696a25fab2c);
		assert_eq!(
			guid,
			Guid {
				data1: 0x436547fc,
				data2: 0xc4ef,
				data3: 0x4322,
				data4: [0xb5, 0x9e, 0xe6, 0x96, 0xa2, 0x5f, 0xab, 0x2c],
			}
		);
	}

	#[test]
	fn guid_is_16_bytes() {
		assert_eq!(size_of::<Guid>(), 16);
		assert_eq!(align_of::<Guid>(), 4);
	}
}
