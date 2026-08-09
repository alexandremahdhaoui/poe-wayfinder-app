use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use thiserror::Error;

use crate::controller::datagen_controller::{
    build_items, build_stats, build_trade_tags, item_to_ndjson, stat_to_ndjson, DatagenError,
};

pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const STAMP: &str = "refreshed-at";

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("building {table} from what the trade api returned")]
    Build {
        table: &'static str,
        #[source]
        source: DatagenError,
    },

    #[error("the trade api returned no {table}")]
    Empty { table: &'static str },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Built {
    pub stats: String,
    pub items: String,
    pub stat_count: usize,
    pub item_count: usize,
}

pub fn refresh_due(last: Option<SystemTime>, now: SystemTime) -> bool {
    let Some(last) = last else {
        return true;
    };

    match now.duration_since(last) {
        Ok(age) => age >= MAX_AGE,
        Err(_) => false,
    }
}

pub fn stamp_path(cache: &Path) -> PathBuf {
    cache.join(STAMP)
}

pub fn last_refresh(cache: &Path) -> Option<SystemTime> {
    std::fs::metadata(stamp_path(cache))
        .ok()
        .and_then(|m| m.modified().ok())
}

pub fn build(stats_body: &str, items_body: &str, static_body: &str) -> Result<Built, RefreshError> {
    let tags = build_trade_tags(static_body).map_err(|source| RefreshError::Build {
        table: "trade tags",
        source,
    })?;

    let stats = build_stats(stats_body).map_err(|source| RefreshError::Build {
        table: "stats",
        source,
    })?;

    let items = build_items(items_body, &tags).map_err(|source| RefreshError::Build {
        table: "items",
        source,
    })?;

    if stats.is_empty() {
        return Err(RefreshError::Empty { table: "stats" });
    }

    if items.is_empty() {
        return Err(RefreshError::Empty { table: "items" });
    }

    Ok(Built {
        stat_count: stats.len(),
        item_count: items.len(),
        stats: to_ndjson(stats.iter().map(stat_to_ndjson)),
        items: to_ndjson(items.iter().map(item_to_ndjson)),
    })
}

fn to_ndjson(lines: impl Iterator<Item = String>) -> String {
    let mut out = String::new();

    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_cache_that_was_never_refreshed_is_due() {
        assert!(refresh_due(None, ago(0)));
    }

    #[test]
    fn a_cache_refreshed_just_now_is_not_due() {
        let now = ago(MAX_AGE.as_secs() * 2);

        assert!(!refresh_due(Some(now), now));
    }

    #[test]
    fn a_cache_older_than_the_window_is_due() {
        let now = ago(MAX_AGE.as_secs() * 2);
        let then = now - MAX_AGE;

        assert!(refresh_due(Some(then), now));
    }

    #[test]
    fn a_cache_inside_the_window_is_not_due() {
        let now = ago(MAX_AGE.as_secs() * 2);
        let then = now - MAX_AGE + Duration::from_secs(1);

        assert!(!refresh_due(Some(then), now));
    }

    #[test]
    fn a_stamp_from_the_future_is_left_alone_rather_than_refreshed_forever() {
        let now = ago(100);
        let later = ago(100) + Duration::from_secs(60);

        assert!(
            !refresh_due(Some(later), now),
            "a clock change must not turn into a refresh on every launch"
        );
    }

    #[test]
    fn the_stamp_lives_beside_the_data_it_describes() {
        let cache = Path::new("/cfg/data-poe2");

        assert_eq!(stamp_path(cache).parent(), Some(cache));
    }

    #[test]
    fn a_cache_with_no_stamp_reports_no_refresh() {
        assert_eq!(last_refresh(Path::new("/nowhere/at/all")), None);
    }

    const STATS: &str = r##"{"result":[{"id":"explicit","label":"Explicit","entries":[{"id":"explicit.stat_3299347043","text":"# to maximum Life","type":"explicit"}]}]}"##;

    const ITEMS: &str = r##"{"result":[{"id":"accessory","label":"Accessories","entries":[{"type":"Sapphire Ring"}]}]}"##;

    const STATIC: &str = r##"{"result":[{"id":"Currency","label":"Currency","entries":[{"id":"chaos","text":"Chaos Orb"}]}]}"##;

    const NOTHING: &str = r##"{"result":[]}"##;

    #[test]
    fn a_good_answer_becomes_two_ndjson_files() {
        let built = build(STATS, ITEMS, STATIC).expect("a build");

        assert!(built.stat_count > 0);
        assert!(built.item_count > 0);
        assert!(built.stats.ends_with('\n'));
        assert!(built.items.ends_with('\n'));

        for line in built.stats.lines().chain(built.items.lines()) {
            assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        }
    }

    #[test]
    fn an_answer_with_no_stats_is_refused_rather_than_written() {
        let err = build(NOTHING, ITEMS, STATIC).expect_err("a refusal");

        assert!(err.to_string().contains("stats"), "{err}");
    }

    #[test]
    fn an_answer_with_no_items_is_refused_rather_than_written() {
        let err = build(STATS, NOTHING, STATIC).expect_err("a refusal");

        assert!(err.to_string().contains("items"), "{err}");
    }

    #[test]
    fn an_answer_with_no_static_table_is_refused_rather_than_written() {
        let err = build(STATS, ITEMS, NOTHING).expect_err("a refusal");

        assert!(err.to_string().contains("trade tags"), "{err}");
    }

    #[test]
    fn a_body_that_is_not_json_names_the_table_it_came_from() {
        let err = build("not json", ITEMS, STATIC).expect_err("a refusal");

        assert!(err.to_string().contains("stats"), "{err}");
    }

    #[test]
    fn an_empty_build_writes_an_empty_string_rather_than_a_blank_line() {
        assert_eq!(to_ndjson(std::iter::empty()), "");
    }
}
