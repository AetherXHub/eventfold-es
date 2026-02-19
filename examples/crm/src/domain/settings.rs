//! Settings aggregate -- application-level preferences.
//!
//! A singleton aggregate (instance ID `"global"`) that stores user preferences
//! such as dark mode. The default state is valid (light mode), so no
//! `created` sentinel is needed.

use eventfold_es::Aggregate;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Application settings persisted in the event store.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Whether dark mode is enabled (`false` = light mode).
    pub dark_mode: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Commands accepted by the [`Settings`] aggregate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SettingsCommand {
    /// Toggle dark mode on or off.
    SetDarkMode {
        /// `true` to enable dark mode, `false` for light mode.
        enabled: bool,
    },
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Domain events produced by the [`Settings`] aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SettingsEvent {
    /// Dark mode preference was changed.
    DarkModeSet {
        /// The new dark mode state.
        enabled: bool,
    },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from [`SettingsCommand`] handling.
///
/// Currently empty -- all settings commands are unconditionally valid.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {}

// ---------------------------------------------------------------------------
// Aggregate impl
// ---------------------------------------------------------------------------

impl Aggregate for Settings {
    const AGGREGATE_TYPE: &'static str = "settings";
    type Command = SettingsCommand;
    type DomainEvent = SettingsEvent;
    type Error = SettingsError;

    fn handle(&self, cmd: SettingsCommand) -> Result<Vec<SettingsEvent>, SettingsError> {
        match cmd {
            SettingsCommand::SetDarkMode { enabled } => {
                Ok(vec![SettingsEvent::DarkModeSet { enabled }])
            }
        }
    }

    fn apply(mut self, event: &SettingsEvent) -> Self {
        match event {
            SettingsEvent::DarkModeSet { enabled } => {
                self.dark_mode = *enabled;
            }
        }
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_light_mode() {
        let s = Settings::default();
        assert!(!s.dark_mode);
    }

    #[test]
    fn set_dark_mode_on() {
        let s = Settings::default();
        let events = s
            .handle(SettingsCommand::SetDarkMode { enabled: true })
            .expect("should succeed");
        let s = events.into_iter().fold(s, |s, e| s.apply(&e));
        assert!(s.dark_mode);
    }

    #[test]
    fn set_dark_mode_off_after_on() {
        let mut s = Settings::default();
        s.dark_mode = true;
        let events = s
            .handle(SettingsCommand::SetDarkMode { enabled: false })
            .expect("should succeed");
        let s = events.into_iter().fold(s, |s, e| s.apply(&e));
        assert!(!s.dark_mode);
    }

    #[test]
    fn idempotent_set() {
        let s = Settings::default();
        let events = s
            .handle(SettingsCommand::SetDarkMode { enabled: false })
            .expect("should succeed even when already false");
        assert_eq!(events.len(), 1);
        let s = events.into_iter().fold(s, |s, e| s.apply(&e));
        assert!(!s.dark_mode);
    }
}
