//! Genres for Spotify tracks, from the descriptors Spotify tags each track with.

use protobuf::CodedInputStream;
use protobuf::rt::WireType;

/// Descriptors Spotify is less sure of than this are left out; its own top genres for a track
/// weigh 0.9 or so, and loosely related ones drop below 0.5.
pub const MIN_WEIGHT: f32 = 0.6;

/// At most this many genres per track, the surest first.
pub const MAX_GENRES: usize = 3;

/// The genres in a `TRACK_DESCRIPTOR` value from Spotify's extended metadata, at most
/// [`MAX_GENRES`] of those weighing [`MIN_WEIGHT`] or more, surest first, with each word
/// capitalized: Spotify sends them in lowercase.
pub fn parse_genres(bytes: &[u8]) -> Vec<String> {
	let mut descriptors = read_descriptors(bytes).unwrap_or_default();
	descriptors.retain(|descriptor| {
		descriptor.genre && descriptor.weight >= MIN_WEIGHT && !descriptor.text.is_empty()
	});
	descriptors.sort_by(|a, b| b.weight.total_cmp(&a.weight));
	let mut genres: Vec<String> = Vec::new();
	for descriptor in descriptors {
		let genre = capitalize(&descriptor.text);
		if !genres.contains(&genre) {
			genres.push(genre);
		}
		if genres.len() == MAX_GENRES {
			break;
		}
	}
	genres
}

/// `ExtensionDescriptorType::GENRE`. A descriptor can have several types, such as genre and mood.
const GENRE: u64 = 1;

struct Descriptor {
	text: String,
	weight: f32,
	genre: bool,
}

/// librespot doesn't compile `ExtensionDescriptorData`, so it is read straight from the
/// protobuf wire format: field 1 holds the descriptors, each with `text` (1), `weight` (2, a
/// float) and its repeated `types` (3), packed or not.
fn read_descriptors(bytes: &[u8]) -> Option<Vec<Descriptor>> {
	let mut input = CodedInputStream::from_bytes(bytes);
	let mut descriptors = Vec::new();
	while let Some(tag) = input.read_raw_tag_or_eof().ok()? {
		match (tag >> 3, tag & 7) {
			(1, 2) => descriptors.push(read_descriptor(&input.read_bytes().ok()?)?),
			(_, wire) => input.skip_field(WireType::new(wire)?).ok()?,
		}
	}
	Some(descriptors)
}

fn read_descriptor(bytes: &[u8]) -> Option<Descriptor> {
	let mut input = CodedInputStream::from_bytes(bytes);
	let mut descriptor = Descriptor { text: String::new(), weight: 0.0, genre: false };
	while let Some(tag) = input.read_raw_tag_or_eof().ok()? {
		match (tag >> 3, tag & 7) {
			(1, 2) => descriptor.text = input.read_string().ok()?,
			(2, 5) => descriptor.weight = input.read_float().ok()?,
			(3, 0) => descriptor.genre |= input.read_raw_varint64().ok()? == GENRE,
			(3, 2) => {
				let packed = input.read_bytes().ok()?;
				let mut types = CodedInputStream::from_bytes(&packed);
				while !types.eof().ok()? {
					descriptor.genre |= types.read_raw_varint64().ok()? == GENRE;
				}
			}
			(_, wire) => input.skip_field(WireType::new(wire)?).ok()?,
		}
	}
	Some(descriptor)
}

/// "celtic folk" becomes "Celtic Folk".
fn capitalize(text: &str) -> String {
	text.split(' ')
		.map(|word| {
			let mut chars = word.chars();
			chars
				.next()
				.map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
		})
		.collect::<Vec<_>>()
		.join(" ")
}

#[cfg(test)]
mod tests {
	use protobuf::CodedOutputStream;

	use super::parse_genres;

	const GENRE: u64 = 1;
	const MOOD: u64 = 2;

	/// An `ExtensionDescriptor`: its text, weight and types, packed as proto3 packs them.
	fn descriptor(text: &str, weight: f32, types: &[u64]) -> Vec<u8> {
		let mut packed = Vec::new();
		let mut out = CodedOutputStream::vec(&mut packed);
		for kind in types {
			out.write_raw_varint64(*kind).unwrap();
		}
		out.flush().unwrap();
		drop(out);
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		out.write_string(1, text).unwrap();
		out.write_float(2, weight).unwrap();
		out.write_bytes(3, &packed).unwrap();
		out.flush().unwrap();
		drop(out);
		bytes
	}

	/// An `ExtensionDescriptorData` holding `descriptors`.
	fn data(descriptors: &[Vec<u8>]) -> Vec<u8> {
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		for descriptor in descriptors {
			out.write_bytes(1, descriptor).unwrap();
		}
		out.flush().unwrap();
		drop(out);
		bytes
	}

	#[test]
	fn keeps_the_three_surest_genres_capitalized() {
		let bytes = data(&[
			descriptor("worship", 0.92, &[GENRE]),
			descriptor("gospel", 0.91, &[GENRE]),
			descriptor("devotional", 0.78, &[GENRE]),
			descriptor("ccm", 0.66, &[GENRE]),
		]);
		assert_eq!(parse_genres(&bytes), ["Worship", "Gospel", "Devotional"]);
	}

	#[test]
	fn leaves_out_genres_below_the_minimum_weight() {
		let bytes = data(&[
			descriptor("classical", 0.90, &[GENRE]),
			descriptor("orchestral", 0.51, &[GENRE]),
		]);
		assert_eq!(parse_genres(&bytes), ["Classical"]);
	}

	#[test]
	fn sorts_by_weight() {
		let bytes =
			data(&[descriptor("hymns", 0.7, &[GENRE]), descriptor("celtic folk", 0.8, &[GENRE])]);
		assert_eq!(parse_genres(&bytes), ["Celtic Folk", "Hymns"]);
	}

	#[test]
	fn leaves_out_moods_and_other_descriptors() {
		let bytes = data(&[
			descriptor("peaceful", 0.95, &[MOOD]),
			descriptor("worship", 0.9, &[GENRE]),
			descriptor("guitar", 0.9, &[]),
		]);
		assert_eq!(parse_genres(&bytes), ["Worship"]);
	}

	#[test]
	fn a_descriptor_that_is_genre_and_mood_counts() {
		let bytes = data(&[descriptor("ambient", 0.8, &[MOOD, GENRE])]);
		assert_eq!(parse_genres(&bytes), ["Ambient"]);
	}

	#[test]
	fn reads_unpacked_types_too() {
		let mut bytes = Vec::new();
		let mut out = CodedOutputStream::vec(&mut bytes);
		out.write_string(1, "gospel").unwrap();
		out.write_float(2, 0.9).unwrap();
		out.write_enum(3, 1).unwrap();
		out.flush().unwrap();
		drop(out);
		assert_eq!(parse_genres(&data(&[bytes])), ["Gospel"]);
	}

	#[test]
	fn leaves_out_empty_and_repeated_genres() {
		let bytes = data(&[
			descriptor("", 0.9, &[GENRE]),
			descriptor("gospel", 0.9, &[GENRE]),
			descriptor("Gospel", 0.8, &[GENRE]),
		]);
		assert_eq!(parse_genres(&bytes), ["Gospel"]);
	}

	#[test]
	fn garbage_has_no_genres() {
		assert!(parse_genres(&[0xff, 0xff, 0xff, 0xff]).is_empty());
		assert!(parse_genres(&[]).is_empty());
	}
}
