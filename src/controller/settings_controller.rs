use crate::adapter::config_store_adapter::{Settings, SettingsStore};

#[cfg_attr(test, mockall::automock)]
pub trait RememberedSettings {
    fn last_league(&self) -> Option<String>;

    fn notes(&self) -> String {
        String::new()
    }

    fn remember_notes(&mut self, _notes: &str) {}

    fn map_verdicts(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn remember_verdict(&mut self, _matcher: &str, _decisions: &str) {}

    fn remember_league(&mut self, league: &str);
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

    fn last_league(&self) -> Option<String> {
        match self.settings.last_league.is_empty() {
            true => None,
            false => Some(self.settings.last_league.clone()),
        }
    }

    fn remember_league(&mut self, league: &str) {
        if league.is_empty() || self.settings.last_league == league {
            return;
        }

        self.settings.last_league = league.to_string();

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
            last_league: league.clone(),
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

        assert_eq!(controller.last_league(), Some("Standard".to_string()));
    }

    #[test]
    fn a_first_run_offers_nothing_rather_than_an_empty_name() {
        let controller = SettingsController::new(store_holding(""));

        assert_eq!(controller.last_league(), None);
    }

    #[test]
    fn a_new_league_is_written_once() {
        let mut store = store_holding("Standard");
        store.expect_save().times(1).returning(|_| Ok(()));

        let mut controller = SettingsController::new(store);

        controller.remember_league("Rise of the Abyssal");

        assert_eq!(
            controller.last_league(),
            Some("Rise of the Abyssal".to_string())
        );
    }

    #[test]
    fn the_same_league_is_not_written_again() {
        let mut store = store_holding("Standard");
        store.expect_save().never();

        SettingsController::new(store).remember_league("Standard");
    }

    #[test]
    fn an_empty_league_is_never_written() {
        let mut store = store_holding("Standard");
        store.expect_save().never();

        SettingsController::new(store).remember_league("");
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

        controller.remember_league("Rise of the Abyssal");

        assert_eq!(
            controller.last_league(),
            Some("Rise of the Abyssal".to_string())
        );
    }
}
