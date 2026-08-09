use crate::adapter::config_store_adapter::{Settings, SettingsStore};

#[cfg_attr(test, mockall::automock)]
pub trait RememberedSettings {
    fn last_league(&self) -> Option<String>;

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
