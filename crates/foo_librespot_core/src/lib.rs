//! Spotify playback for foo_librespot, built on librespot.
//!
//! This crate knows nothing about foobar2000: it turns Spotify links into track lists with
//! metadata and plays Spotify tracks as seekable 44.1 kHz stereo PCM.

pub mod error;
pub mod link;

pub use error::{CoreError, Result};
pub use librespot_core::{FileId, SpotifyId, SpotifyUri};
