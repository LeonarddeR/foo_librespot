//! foo_librespot's entries in Advanced preferences: a Spotify branch under Tools, holding the
//! switch for detailed logging.

use foxide2k::abi::advconfig::GUID_BRANCH_TOOLS;
use foxide2k::advconfig::{AdvconfigBranch, AdvconfigCheckbox};
use foxide2k::guid::Guid;

use crate::{logging, settings};

/// Advanced > Tools > Spotify.
pub struct SpotifyBranch;

impl AdvconfigBranch for SpotifyBranch {
	const GUID: Guid = Guid::from_u128(0x131920cd_ef5c_4d81_8c36_9f4736ee9aea);
	const PARENT: Guid = GUID_BRANCH_TOOLS;
	const NAME: &'static str = "Spotify";
}

/// The switch for detailed logging.
pub struct DetailedLogging;

impl AdvconfigCheckbox for DetailedLogging {
	const GUID: Guid = Guid::from_u128(0xcf7aa8cf_1a02_479f_99c3_67e589d31bcb);
	const PARENT: Guid = SpotifyBranch::GUID;
	const NAME: &'static str = "Log details to the console, for bug reports";
	const DEFAULT: bool = false;

	fn state() -> bool {
		settings::detailed_logging()
	}

	fn set_state(state: bool) -> foxide2k::error::Result<()> {
		settings::set_detailed_logging(state)?;
		logging::set_detailed(state);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use foxide2k::abi::advconfig::GUID_BRANCH_TOOLS;
	use foxide2k::advconfig::{advconfig_branch_service, advconfig_checkbox_service};
	use foxide2k::testing::{AdvconfigHandle, FakeApi};
	use log::LevelFilter;

	use super::{DetailedLogging, SpotifyBranch};
	use crate::settings::DETAILED_LOGGING_KEY;

	fn branch() -> AdvconfigHandle {
		AdvconfigHandle::new(advconfig_branch_service::<SpotifyBranch>())
	}

	fn checkbox() -> AdvconfigHandle {
		AdvconfigHandle::new(advconfig_checkbox_service::<DetailedLogging>())
	}

	#[test]
	fn the_logging_checkbox_sits_in_a_spotify_branch_under_tools() {
		let branch = branch();
		assert_eq!(branch.name(), "Spotify");
		assert_eq!(branch.parent(), GUID_BRANCH_TOOLS);
		let checkbox = checkbox();
		assert_eq!(checkbox.name(), "Log details to the console, for bug reports");
		assert_eq!(checkbox.parent(), branch.guid());
	}

	#[test]
	fn the_logging_checkbox_is_off_by_default() {
		FakeApi::new().install();
		let checkbox = checkbox();
		assert!(!checkbox.state());
		assert!(!checkbox.default_state());
	}

	#[test]
	fn checking_the_box_stores_it_and_logs_details_at_once() {
		let api = FakeApi::new();
		api.install();
		assert!(checkbox().set_state(true).is_ok());
		assert_eq!(api.config_int(DETAILED_LOGGING_KEY), Some(1));
		assert_eq!(log::max_level(), LevelFilter::Debug);
	}

	#[test]
	fn resetting_the_box_turns_detailed_logging_off() {
		FakeApi::new().install();
		let checkbox = checkbox();
		checkbox.set_state(true).unwrap();
		checkbox.reset();
		assert!(!checkbox.state());
		assert_eq!(log::max_level(), LevelFilter::Info);
	}
}
