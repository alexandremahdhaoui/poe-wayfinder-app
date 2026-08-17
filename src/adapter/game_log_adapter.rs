use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LogError {
    #[error("opening {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("reading {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    EnteredArea {
        name: String,
    },
    LevelUp {
        character: String,
        class: String,
        level: u32,
    },
    Whisper {
        from: String,
        body: String,
    },
}

pub fn parse_log_line(line: &str) -> Option<LogEvent> {
    if let Some(rest) = line.split_once("] : You have entered ") {
        let name = rest.1.trim().trim_end_matches('.');

        if name.is_empty() {
            return None;
        }

        return Some(LogEvent::EnteredArea {
            name: name.to_string(),
        });
    }

    if let Some(event) = parse_level_up(line) {
        return Some(event);
    }

    parse_whisper(line)
}

fn parse_level_up(line: &str) -> Option<LogEvent> {
    let payload = line.split_once("] : ")?.1;

    let (name_and_class, level) = payload.rsplit_once(" is now level ")?;
    let level: u32 = level.trim().parse().ok()?;

    let (character, class) = name_and_class.rsplit_once(" (")?;
    let class = class.strip_suffix(')')?;

    if character.trim().is_empty() || class.trim().is_empty() {
        return None;
    }

    Some(LogEvent::LevelUp {
        character: character.trim().to_string(),
        class: class.to_string(),
        level,
    })
}

fn parse_whisper(line: &str) -> Option<LogEvent> {
    let payload = line.split_once("] ")?.1;

    let rest = payload.strip_prefix("@From ")?;

    let rest = match rest.strip_prefix('<') {
        Some(after) => after.split_once("> ")?.1,
        None => rest,
    };

    let (from, body) = rest.split_once(": ")?;

    if from.trim().is_empty() {
        return None;
    }

    Some(LogEvent::Whisper {
        from: from.trim().to_string(),
        body: body.to_string(),
    })
}

pub fn league_from_whisper(body: &str) -> Option<String> {
    let (_, after) = body.rsplit_once(" in ")?;

    let league = after
        .split(&['(', ')'][..])
        .next()?
        .trim()
        .trim_end_matches(['.', ',']);

    if league.is_empty() || league.len() > 40 {
        return None;
    }

    Some(league.to_string())
}

#[cfg_attr(test, mockall::automock)]
pub trait LogReader: Send + Sync {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError>;

    fn watch(&mut self, _path: &Path) {}
}

pub struct NoLog;

impl LogReader for NoLog {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        Ok(Vec::new())
    }
}

impl LogReader for GameLogWatcher {
    fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        GameLogWatcher::poll(self)
    }

    fn watch(&mut self, path: &Path) {
        GameLogWatcher::watch(self, path);
    }
}

#[derive(Debug)]
pub struct GameLogWatcher {
    path: PathBuf,
    position: u64,
}

impl GameLogWatcher {
    pub fn new(path: &Path) -> Self {
        let position = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        Self {
            path: path.to_path_buf(),
            position,
        }
    }

    pub fn from_start(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            position: 0,
        }
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn watch(&mut self, path: &Path) {
        if path == self.path {
            return;
        }

        self.path = path.to_path_buf();
        self.position = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }

    pub fn poll(&mut self) -> Result<Vec<LogEvent>, LogError> {
        let Ok(file) = std::fs::File::open(&self.path) else {
            return Ok(Vec::new());
        };

        let length = file
            .metadata()
            .map_err(|source| LogError::Read {
                path: self.path.clone(),
                source,
            })?
            .len();

        if length < self.position {
            self.position = 0;
        }

        if length == self.position {
            return Ok(Vec::new());
        }

        let mut reader = BufReader::new(file);

        reader
            .seek(SeekFrom::Start(self.position))
            .map_err(|source| LogError::Read {
                path: self.path.clone(),
                source,
            })?;

        let mut out = Vec::new();
        let mut consumed = self.position;

        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => break,
            };

            consumed += line.len() as u64 + 1;

            if let Some(event) = parse_log_line(&line) {
                out.push(event);
            }
        }

        self.position = consumed.min(length);

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempfile(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poe-wayfinder-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        dir.join(name)
    }

    fn write(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    const AREA: &str =
        "2026/08/04 12:00:00 123456 cffb0716 [INFO Client 1234] : You have entered Sarn Encampment.";
    const LEVEL: &str =
        "2026/08/04 12:00:00 123456 cffb0716 [INFO Client 1234] : Kaom (Marauder) is now level 42";
    const WHISPER: &str = "2026/08/04 12:00:00 123456 cffb0716 [INFO Client 1234] @From Kaom: Hi, I would like to buy your Sapphire Ring listed for 5 divine in Standard";

    #[test]
    fn an_area_line_is_read() {
        assert_eq!(
            parse_log_line(AREA),
            Some(LogEvent::EnteredArea {
                name: "Sarn Encampment".into()
            })
        );
    }

    #[test]
    fn the_trailing_full_stop_is_not_part_of_the_area_name() {
        let event = parse_log_line(AREA).unwrap();

        match event {
            LogEvent::EnteredArea { name } => assert!(!name.ends_with('.')),
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn a_level_up_line_names_the_character_and_class() {
        assert_eq!(
            parse_log_line(LEVEL),
            Some(LogEvent::LevelUp {
                character: "Kaom".into(),
                class: "Marauder".into(),
                level: 42
            })
        );
    }

    #[test]
    fn a_character_name_with_a_space_survives() {
        let line = "2026/08/04 12:00:00 1 a [INFO Client 1] : Big Kaom (Marauder) is now level 5";

        assert_eq!(
            parse_log_line(line),
            Some(LogEvent::LevelUp {
                character: "Big Kaom".into(),
                class: "Marauder".into(),
                level: 5
            })
        );
    }

    #[test]
    fn a_whisper_is_read() {
        let event = parse_log_line(WHISPER).unwrap();

        match event {
            LogEvent::Whisper { from, body } => {
                assert_eq!(from, "Kaom");
                assert!(body.contains("Sapphire Ring"));
            }
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn a_guild_tag_is_not_part_of_the_whisperer_name() {
        let line = "2026/08/04 12:00:00 1 a [INFO Client 1] @From <TAG> Kaom: hello";

        match parse_log_line(line).unwrap() {
            LogEvent::Whisper { from, .. } => assert_eq!(from, "Kaom"),
            other => panic!("wrong event: {other:?}"),
        }
    }

    #[test]
    fn an_outgoing_whisper_is_not_read_as_incoming() {
        let line = "2026/08/04 12:00:00 1 a [INFO Client 1] @To Kaom: hello";

        assert_eq!(parse_log_line(line), None);
    }

    #[test]
    fn ordinary_engine_noise_yields_nothing() {
        for line in [
            "2026/08/04 12:00:00 1 a [DEBUG Client 1] Got Instance Details",
            "2026/08/04 12:00:00 1 a [INFO Client 1] Connecting to instance server",
            "",
            "not a log line at all",
        ] {
            assert_eq!(parse_log_line(line), None, "{line}");
        }
    }

    #[test]
    fn a_malformed_level_up_is_not_read() {
        for line in [
            "2026/08/04 12:00:00 1 a [INFO Client 1] : is now level 42",
            "2026/08/04 12:00:00 1 a [INFO Client 1] : Kaom (Marauder) is now level abc",
            "2026/08/04 12:00:00 1 a [INFO Client 1] : Kaom is now level 42",
        ] {
            assert_eq!(parse_log_line(line), None, "{line}");
        }
    }

    #[test]
    fn an_empty_area_name_is_not_read() {
        let line = "2026/08/04 12:00:00 1 a [INFO Client 1] : You have entered ";

        assert_eq!(parse_log_line(line), None);
    }

    #[test]
    fn the_league_is_pulled_out_of_a_trade_whisper() {
        let body = "Hi, I would like to buy your Sapphire Ring listed for 5 divine in Standard";

        assert_eq!(league_from_whisper(body), Some("Standard".into()));
    }

    #[test]
    fn a_league_with_a_space_survives() {
        let body = "Hi, I would like to buy your item listed for 1 chaos in Hardcore Ruthless";

        assert_eq!(league_from_whisper(body), Some("Hardcore Ruthless".into()));
    }

    #[test]
    fn a_stash_tab_note_after_the_league_is_dropped() {
        let body = "Hi, I want your item listed for 5 divine in Standard (stash tab \"A\"; position: left 1, top 2)";

        assert_eq!(league_from_whisper(body), Some("Standard".into()));
    }

    #[test]
    fn a_whisper_that_names_no_league_yields_nothing() {
        assert_eq!(league_from_whisper("hello there"), None);
        assert_eq!(league_from_whisper(""), None);
    }

    #[test]
    fn an_absurdly_long_league_name_is_rejected() {
        let body = format!("buying your item in {}", "x".repeat(100));

        assert_eq!(league_from_whisper(&body), None);
    }

    #[test]
    fn a_new_watcher_starts_at_the_end() {
        let path = tempfile("start-at-end.txt");
        write(&path, &format!("{AREA}\n"));

        let mut w = GameLogWatcher::new(&path);

        assert!(w.poll().unwrap().is_empty());
    }

    #[test]
    fn new_lines_are_reported() {
        let path = tempfile("new-lines.txt");
        write(&path, "old\n");

        let mut w = GameLogWatcher::new(&path);
        append(&path, &format!("{AREA}\n"));

        let events = w.poll().unwrap();

        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_line_is_reported_only_once() {
        let path = tempfile("once.txt");
        write(&path, "old\n");

        let mut w = GameLogWatcher::new(&path);
        append(&path, &format!("{AREA}\n"));

        assert_eq!(w.poll().unwrap().len(), 1);
        assert!(w.poll().unwrap().is_empty());
    }

    #[test]
    fn a_truncated_log_is_read_from_the_start_again() {
        let path = tempfile("truncate.txt");
        write(&path, &format!("{AREA}\n{AREA}\n{AREA}\n"));

        let mut w = GameLogWatcher::from_start(&path);
        assert_eq!(w.poll().unwrap().len(), 3);

        write(&path, &format!("{LEVEL}\n"));

        let events = w.poll().unwrap();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], LogEvent::LevelUp { .. }));
    }

    #[test]
    fn watching_a_new_file_moves_the_watcher_and_skips_what_is_already_there() {
        let first = tempfile("watch-first.txt");
        let second = tempfile("watch-second.txt");

        write(&first, &format!("{AREA}\n"));
        write(&second, &format!("{LEVEL}\n"));

        let mut w = GameLogWatcher::from_start(&first);

        assert_eq!(w.poll().unwrap().len(), 1);

        w.watch(&second);

        assert!(
            w.poll().unwrap().is_empty(),
            "the other game's backlog must not be replayed as if it just happened"
        );

        write(&second, &format!("{LEVEL}\n{AREA}\n"));

        assert_eq!(w.poll().unwrap().len(), 1);
    }

    #[test]
    fn watching_the_file_it_already_watches_does_not_rewind_it() {
        let path = tempfile("watch-same.txt");

        write(&path, &format!("{AREA}\n"));

        let mut w = GameLogWatcher::from_start(&path);

        assert_eq!(w.poll().unwrap().len(), 1);

        let at = w.position();

        w.watch(&path);

        assert_eq!(w.position(), at);
        assert!(w.poll().unwrap().is_empty());
    }

    #[test]
    fn a_whole_log_is_parsed_line_by_line() {
        let text = format!("{AREA}\nnot a log line\n{LEVEL}\n");

        let found = text.lines().filter_map(parse_log_line).count();

        assert_eq!(found, 2, "the noise between is skipped");
    }

    #[test]
    fn an_empty_line_parses_to_nothing() {
        assert!(parse_log_line("").is_none());
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let mut w = GameLogWatcher::new(Path::new("/nonexistent/Client.txt"));

        assert!(w.poll().unwrap().is_empty());
    }

    #[test]
    fn an_unchanged_file_reports_nothing() {
        let path = tempfile("unchanged.txt");
        write(&path, &format!("{AREA}\n"));

        let mut w = GameLogWatcher::from_start(&path);
        w.poll().unwrap();

        let before = w.position();

        assert!(w.poll().unwrap().is_empty());
        assert_eq!(w.position(), before);
    }

    #[test]
    fn every_event_type_is_read_from_a_real_looking_log() {
        let path = tempfile("mixed.txt");
        write(
            &path,
            &format!("noise\n{AREA}\nmore noise\n{LEVEL}\n{WHISPER}\n"),
        );

        let mut w = GameLogWatcher::from_start(&path);
        let events = w.poll().unwrap();

        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], LogEvent::EnteredArea { .. }));
        assert!(matches!(events[1], LogEvent::LevelUp { .. }));
        assert!(matches!(events[2], LogEvent::Whisper { .. }));
    }
}
