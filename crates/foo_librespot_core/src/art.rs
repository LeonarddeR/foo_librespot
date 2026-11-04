//! Album art for Spotify tracks.

use librespot_core::Session;

use crate::metadata::Cover;
use crate::{CoreError, FileId};

/// Spotify's public image server, which librespot also falls back to.
pub fn fallback_image_url(file_id: &FileId) -> String {
	format!("https://i.scdn.co/image/{}", file_id.to_base16())
}

/// Downloads one cover image, as JPEG bytes.
pub async fn fetch_cover(session: &Session, cover: &Cover) -> Result<Vec<u8>, CoreError> {
	match session.spclient().get_image(&cover.file_id).await {
		Ok(bytes) => Ok(bytes.to_vec()),
		Err(error) => {
			log::debug!("cover through the image-url attribute failed ({error}); trying i.scdn.co");
			let url = fallback_image_url(&cover.file_id);
			Ok(session.spclient().request_url(&url).await?.to_vec())
		}
	}
}

#[cfg(test)]
mod tests {
	use super::fallback_image_url;
	use crate::FileId;

	#[test]
	fn fallback_image_url_uses_base16() {
		let url = fallback_image_url(&FileId::from_raw(&[0xab; 20]));
		assert_eq!(url, format!("https://i.scdn.co/image/{}", "ab".repeat(20)));
	}
}
