use crate::adapter::game_log_adapter::{GameLogWatcher, LogError, LogEvent};

#[cfg_attr(test, mockall::automock)]
pub trait LogSource {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError>;
}

pub struct FileLogSource {
    watcher: GameLogWatcher,
}

impl FileLogSource {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            watcher: GameLogWatcher::new(path),
        }
    }
}

impl LogSource for FileLogSource {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        self.watcher.poll()
    }
}

pub struct NoLogSource;

impl LogSource for NoLogSource {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        Ok(Vec::new())
    }
}

impl LogSource for Box<dyn LogSource> {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        (**self).poll()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_log_yields_nothing_rather_than_failing() {
        assert!(NoLogSource.poll().expect("no error").is_empty());
    }

    #[test]
    fn events_reach_the_caller_in_order() {
        let mut source = MockLogSource::new();
        source.expect_poll().times(1).returning(|| {
            Ok(vec![
                LogEvent::EnteredArea {
                    name: "The Twilight Strand".to_string(),
                },
                LogEvent::Whisper {
                    from: "Exile".to_string(),
                    body: "hi".to_string(),
                },
            ])
        });

        let got = source.poll().expect("events");

        assert_eq!(got.len(), 2);
    }
}
