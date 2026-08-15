use poe_wayfinder_core::types::GameVersion;

use crate::adapter::config_store_adapter::{Settings, SettingsStore};

#[cfg_attr(test, mockall::automock)]
pub trait RememberedSettings {
    fn last_league(&self, game: GameVersion) -> Option<String>;

    fn notes(&self) -> String {
        String::new()
    }

    fn remember_notes(&mut self, _notes: &str) {}

    fn map_verdicts(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn remember_verdict(&mut self, _matcher: &str, _decisions: &str) {}

    fn remember_league(&mut self, game: GameVersion, league: &str);

    fn league_is_pinned(&self, _game: GameVersion) -> bool {
        false
    }

    fn pin_league(&mut self, _game: GameVersion, _pinned: bool) {}
}

pub struct SettingsController<S: SettingsStore> {
    store: S,
    settings: Settings,
}

impl<S: SettingsStore> SettingsController<S> {
    pub fn new(store: S) -> Self {
        let settings = store.load();

        Self { store, settings }
    }
}

impl<S: SettingsStore> RememberedSettings for SettingsController<S> {
    fn notes(&self) -> String {
        self.settings.notes.clone()
    }

    fn map_verdicts(&self) -> Vec<(String, String)> {
        self.settings.map_verdicts.clone()
    }

    fn remember_verdict(&mut self, matcher: &str, decisions: &str) {
        if matcher.trim().is_empty() {
            return;
        }

        match self
            .settings
            .map_verdicts
            .iter_mut()
            .find(|(known, _)| known == matcher)
        {
            Some(entry) => entry.1 = decisions.to_string(),
            None => self
                .settings
                .map_verdicts
                .push((matcher.to_string(), decisions.to_string())),
        }

        let _ = self.store.save(&self.settings);
    }

    fn remember_notes(&mut self, notes: &str) {
        if self.settings.notes == notes {
            return;
        }

        self.settings.notes = notes.to_string();

        let _ = self.store.save(&self.settings);
    }

    fn last_league(&self, game: GameVersion) -> Option<String> {
        let known = self.settings.last_league.get(game);

        match known.is_empty() {
            true => None,
            false => Some(known.to_string()),
        }
    }

    fn remember_league(&mut self, game: GameVersion, league: &str) {
        if league.is_empty() || self.settings.last_league.get(game) == league {
            return;
        }

        self.settings.last_league.set(game, league);

        let _ = self.store.save(&self.settings);
    }

    fn league_is_pinned(&self, game: GameVersion) -> bool {
        *self.settings.pinned_leagues.get(game)
    }

    fn pin_league(&mut self, game: GameVersion, pinned: bool) {
        if *self.settings.pinned_leagues.get(game) == pinned {
            return;
        }

        *self.settings.pinned_leagues.get_mut(game) = pinned;

        let _ = self.store.save(&self.settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::config_store_adapter::MockSettingsStore;

    fn store_holding(league: &str) -> MockSettingsStore {
        let league = league.to_string();
        let mut store = MockSettingsStore::new();

        store.expect_load().returning(move || Settings {
            last_league: crate::adapter::config_store_adapter::LastLeague::Shared(league.clone()),
            ..Settings::default()
        });

        store
    }

    #[test]
    fn a_map_verdict_survives_a_restart() {
        let mut store = MockSettingsStore::new();

        store.expect_load().returning(Settings::default);
        store.expect_save().returning(|_| Ok(()));

        let mut settings = SettingsController::new(store);

        settings.remember_verdict("extra fire damage", "d--");

        assert_eq!(
            settings.map_verdicts(),
            vec![("extra fire damage".to_string(), "d--".to_string())]
        );
    }

    #[test]
    fn marking_the_same_mod_again_replaces_the_verdict_rather_than_adding_a_second() {
        let mut store = MockSettingsStore::new();

        store.expect_load().returning(Settings::default);
        store.expect_save().returning(|_| Ok(()));

        let mut settings = SettingsController::new(store);

        settings.remember_verdict("extra fire damage", "d--");
        settings.remember_verdict("extra fire damage", "w--");

        assert_eq!(settings.map_verdicts().len(), 1);
        assert_eq!(settings.map_verdicts()[0].1, "w--");
    }

    #[test]
    fn a_verdict_on_nothing_is_not_stored() {
        let mut store = MockSettingsStore::new();

        store.expect_load().returning(Settings::default);

        let mut settings = SettingsController::new(store);

        settings.remember_verdict("  ", "d--");

        assert!(settings.map_verdicts().is_empty());
    }

    #[test]
    fn notes_survive_a_restart() {
        let mut store = MockSettingsStore::new();

        store.expect_load().returning(Settings::default);
        store.expect_save().returning(|_| Ok(()));

        let mut settings = SettingsController::new(store);

        settings.remember_notes("chaos recipe 60-74");

        assert_eq!(settings.notes(), "chaos recipe 60-74");
    }

    #[test]
    fn a_league_from_a_previous_run_is_offered() {
        let controller = SettingsController::new(store_holding("Standard"));

        assert_eq!(
            controller.last_league(GameVersion::Poe2),
            Some("Standard".to_string())
        );
    }

    #[test]
    fn a_first_run_offers_nothing_rather_than_an_empty_name() {
        let controller = SettingsController::new(store_holding(""));

        assert_eq!(controller.last_league(GameVersion::Poe2), None);
    }

    #[test]
    fn a_new_league_is_written_once() {
        let mut store = store_holding("Standard");
        store.expect_save().times(1).returning(|_| Ok(()));

        let mut controller = SettingsController::new(store);

        controller.remember_league(GameVersion::Poe2, "Rise of the Abyssal");

        assert_eq!(
            controller.last_league(GameVersion::Poe2),
            Some("Rise of the Abyssal".to_string())
        );
    }

    #[test]
    fn a_league_learned_for_one_game_never_becomes_the_other_game_s_league() {
        let mut store = store_holding("");
        store.expect_save().returning(|_| Ok(()));

        let mut controller = SettingsController::new(store);

        controller.remember_league(GameVersion::Poe2, "Rise of the Abyssal");

        assert_eq!(controller.last_league(GameVersion::Poe1), None);

        controller.remember_league(GameVersion::Poe1, "Mercenaries");

        assert_eq!(
            controller.last_league(GameVersion::Poe1),
            Some("Mercenaries".to_string())
        );
        assert_eq!(
            controller.last_league(GameVersion::Poe2),
            Some("Rise of the Abyssal".to_string())
        );
    }

    #[test]
    fn the_same_league_is_not_written_again() {
        let mut store = store_holding("Standard");
        store.expect_save().never();

        SettingsController::new(store).remember_league(GameVersion::Poe2, "Standard");
    }

    #[test]
    fn an_empty_league_is_never_written() {
        let mut store = store_holding("Standard");
        store.expect_save().never();

        SettingsController::new(store).remember_league(GameVersion::Poe2, "");
    }

    #[test]
    fn a_league_pinned_for_one_game_leaves_the_other_game_following_the_trade_site() {
        let mut store = store_holding("Standard");
        store.expect_save().returning(|_| Ok(()));

        let mut controller = SettingsController::new(store);

        assert!(!controller.league_is_pinned(GameVersion::Poe2));

        controller.pin_league(GameVersion::Poe2, true);

        assert!(controller.league_is_pinned(GameVersion::Poe2));
        assert!(!controller.league_is_pinned(GameVersion::Poe1));
    }

    #[test]
    fn pinning_what_is_already_pinned_is_not_written_again() {
        let mut store = store_holding("Standard");
        store.expect_save().never();

        SettingsController::new(store).pin_league(GameVersion::Poe2, false);
    }

    #[test]
    fn handing_a_league_back_to_automatic_is_written_so_the_restart_re_resolves_it() {
        let mut store = store_holding("Standard");
        store.expect_save().times(2).returning(|_| Ok(()));

        let mut controller = SettingsController::new(store);

        controller.pin_league(GameVersion::Poe1, true);
        controller.pin_league(GameVersion::Poe1, false);

        assert!(!controller.league_is_pinned(GameVersion::Poe1));
    }

    #[test]
    fn a_store_that_cannot_write_does_not_stop_the_overlay() {
        let mut store = store_holding("Standard");
        store.expect_save().returning(|_| {
            Err(crate::adapter::config_store_adapter::StoreError::Write {
                path: std::path::PathBuf::from("settings.json"),
                source: std::io::Error::other("read only"),
            })
        });

        let mut controller = SettingsController::new(store);

        controller.remember_league(GameVersion::Poe2, "Rise of the Abyssal");

        assert_eq!(
            controller.last_league(GameVersion::Poe2),
            Some("Rise of the Abyssal".to_string())
        );
    }
}
