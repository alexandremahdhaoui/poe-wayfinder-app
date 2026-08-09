use crate::adapter::game_log_adapter::{LogError, LogEvent, LogReader};

#[cfg_attr(test, mockall::automock)]
pub trait LogSource {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError>;

    fn watch(&mut self, _path: &std::path::Path) {}
}

pub struct LogWatchController<R: LogReader> {
    reader: R,
}

impl<R: LogReader> LogWatchController<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }
}

impl<R: LogReader> LogSource for LogWatchController<R> {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        self.reader.poll()
    }

    fn watch(&mut self, path: &std::path::Path) {
        self.reader.watch(path);
    }
}

impl LogSource for Box<dyn LogSource> {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        (**self).poll()
    }

    fn watch(&mut self, path: &std::path::Path) {
        (**self).watch(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::adapter::game_log_adapter::MockLogReader;

    #[test]
    fn events_reach_the_caller_unchanged() {
        let mut reader = MockLogReader::new();
        reader.expect_poll().times(1).returning(|| {
            Ok(vec![LogEvent::EnteredArea {
                name: "The Twilight Strand".to_string(),
            }])
        });

        let got = LogWatchController::new(reader).poll().expect("events");

        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_reader_that_fails_does_not_panic_the_caller() {
        let mut reader = MockLogReader::new();
        reader.expect_poll().times(1).returning(|| {
            Err(LogError::Open {
                path: std::path::PathBuf::from("Client.txt"),
                source: std::io::Error::other("gone"),
            })
        });

        assert!(LogWatchController::new(reader).poll().is_err());
    }
}
