//! Spotify playback for foo_librespot, built on librespot.
//!
//! This crate knows nothing about foobar2000: it turns Spotify links into track lists with
//! metadata and plays Spotify tracks as seekable 44.1 kHz stereo PCM.

pub mod art;
pub mod audio;
#[allow(
	non_snake_case,
	non_camel_case_types,
	non_upper_case_globals,
	dead_code,
	clippy::all,
	clippy::pedantic,
	clippy::nursery
)]
mod bindings;
mod dpapi;
pub mod error;
pub mod genre;
pub mod library;
pub mod link;
pub mod login;
pub mod metadata;
pub mod profile;
pub mod related;
pub mod replaygain;
pub mod resolve;
pub mod runtime;
pub mod session;
pub mod spotify;

pub use error::{CoreError, Result};
pub use librespot_core::authentication::Credentials;
pub use librespot_core::{FileId, SpotifyId, SpotifyUri};
pub use spotify::{Core, LoginHandle};
