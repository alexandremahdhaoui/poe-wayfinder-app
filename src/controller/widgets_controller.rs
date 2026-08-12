use poe_wayfinder_core::controller::background::{Background, Happening};
use poe_wayfinder_core::controller::help::{entries, widest_key, Entry};
use poe_wayfinder_core::controller::item_search::{search, Hit, Starred};
use poe_wayfinder_core::controller::leveling::{step_for, upcoming, Step};
use poe_wayfinder_core::controller::library::{caption, Library, Logged};
use poe_wayfinder_core::controller::map_check::{
    headline, review, set_verdict, worst, Concern, Verdict, NO_DECISION,
};
use poe_wayfinder_core::controller::notepad::Notepad;
use poe_wayfinder_core::controller::stopwatch::Stopwatch;
use poe_wayfinder_core::types::ParsedItem;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Status,
    Library,
    Maps,
    Search,
    Notes,
    Log,
    Leveling,
    Settings,
    Help,
}

impl Tab {
    pub fn title(self) -> &'static str {
        match self {
            Tab::Status => "Status",
            Tab::Library => "Library",
            Tab::Maps => "Maps",
            Tab::Search => "Search",
            Tab::Notes => "Notes",
            Tab::Log => "Log",
            Tab::Leveling => "Leveling",
            Tab::Settings => "Settings",
            Tab::Help => "Help",
        }
    }

    pub fn every() -> [Tab; 9] {
        [
            Tab::Status,
            Tab::Library,
            Tab::Maps,
            Tab::Search,
            Tab::Notes,
            Tab::Log,
            Tab::Leveling,
            Tab::Settings,
            Tab::Help,
        ]
    }
}

#[derive(Debug, Default)]
pub struct Widgets {
    pub tab: Tab,
    pub library: Library,
    pub notepad: Notepad,
    pub stopwatch: Stopwatch,
    pub needle: String,
    pub log_lines: Vec<String>,
    pub concerns: Vec<Concern>,
    pub verdicts: Vec<(String, String)>,
    pub background: Background,
    pub starred: Starred,
    hidden: Vec<Tab>,
}

pub const LOG_LINES_KEPT: usize = 200;

fn trimmed(amount: f64) -> String {
    match amount.fract() == 0.0 {
        true => format!("{}", amount as i64),
        false => format!("{amount:.2}"),
    }
}

impl Widgets {
    pub fn show(&mut self, tab: Tab) {
        if self.is_enabled(tab) {
            self.tab = tab;
        }
    }

    pub fn find_widget(&self, title: &str) -> Option<Tab> {
        Tab::every().into_iter().find(|tab| tab.title() == title)
    }

    pub fn is_enabled(&self, tab: Tab) -> bool {
        !self.hidden.contains(&tab)
    }

    pub fn enable_widget(&mut self, tab: Tab) {
        self.hidden.retain(|hidden| *hidden != tab);
    }

    pub fn disable_widget(&mut self, tab: Tab) {
        if tab == Tab::Status || self.hidden.contains(&tab) {
            return;
        }

        self.hidden.push(tab);

        if self.tab == tab {
            self.tab = Tab::Status;
        }
    }

    pub fn enabled(&self) -> Vec<Tab> {
        Tab::every()
            .into_iter()
            .filter(|tab| self.is_enabled(*tab))
            .collect()
    }

    pub fn record(&mut self, entry: Logged) {
        self.library.record(entry);
    }

    pub fn totals(&self) -> Vec<String> {
        self.library
            .currencies()
            .into_iter()
            .map(|currency| {
                let total = self.library.total_in(&currency);

                format!("{} {currency}", trimmed(total))
            })
            .collect()
    }

    pub fn open_notes(&mut self, text: &str) {
        self.notepad = Notepad::opened_with(text);
    }

    pub fn cycle_verdict(&mut self, index: usize) -> Option<(String, String)> {
        let known = self
            .concerns
            .get(index)
            .and_then(|concern| {
                self.verdicts
                    .iter()
                    .find(|(matcher, _)| *matcher == concern.text)
                    .map(|(_, set)| set.clone())
            })
            .unwrap_or_else(|| NO_DECISION.to_string());

        let concern = self.concerns.get_mut(index)?;

        concern.verdict = concern.verdict.next();

        let set = set_verdict(&known, 1, concern.verdict);
        let matcher = concern.text.clone();

        match self
            .verdicts
            .iter_mut()
            .find(|(known, _)| *known == matcher)
        {
            Some(entry) => entry.1 = set.clone(),
            None => self.verdicts.push((matcher.clone(), set.clone())),
        }

        Some((matcher, set))
    }

    pub fn remember_verdicts(&mut self, verdicts: Vec<(String, String)>) {
        self.verdicts = verdicts;
    }

    pub fn note_happening(&mut self, happening: &Happening) {
        if !self.background.apply(happening) {
            return;
        }

        if happening.is_worth_showing() {
            self.note_log(&happening.line());
        }
    }

    pub fn levelling_step(&self) -> Option<Step> {
        step_for(self.background.level.unwrap_or(0))
    }

    pub fn levelling_next(&self) -> Vec<Step> {
        upcoming(self.background.level.unwrap_or(0), 3)
    }

    pub fn marked(&self) -> usize {
        self.concerns
            .iter()
            .filter(|c| c.verdict.is_coloured())
            .count()
    }

    pub fn reset_timer(&mut self) {
        self.stopwatch.reset();
    }

    pub fn logged(&self) -> Vec<String> {
        self.library.entries().iter().map(caption).collect()
    }

    pub fn note_line(&self) -> String {
        format!(
            "{} lines{}",
            self.notepad.line_count(),
            match self.notepad.is_dirty() {
                true => ", unsaved",
                false => "",
            }
        )
    }

    pub fn hits(&self, names: &[String]) -> Vec<Hit> {
        search(&self.needle, names)
    }

    pub fn note_log(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }

        self.log_lines.push(line.to_string());

        if self.log_lines.len() > LOG_LINES_KEPT {
            let cut = self.log_lines.len() - LOG_LINES_KEPT;

            self.log_lines.drain(..cut);
        }
    }

    pub fn check_map(&mut self, item: &ParsedItem, profile: usize) {
        let decisions = self.verdicts.clone();

        self.concerns = review(item, &decisions, profile);
    }

    pub fn map_headline(&self) -> String {
        headline(&self.concerns)
    }

    pub fn map_verdict(&self) -> Verdict {
        worst(&self.concerns)
    }

    pub fn help(&self, bindings: &[(String, String)]) -> (Vec<Entry>, usize) {
        let listed = entries(bindings);
        let width = widest_key(&listed);

        (listed, width)
    }

    pub fn elapsed(&self, now_ms: u64) -> String {
        self.stopwatch.reading(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logged(name: &str) -> Logged {
        Logged {
            name: name.to_string(),
            amount: Some(3.0),
            currency: "chaos".to_string(),
            listings: 1,
            at_ms: 0,
        }
    }

    #[test]
    fn every_tab_has_a_title_and_they_are_all_different() {
        let titles: Vec<&str> = Tab::every().iter().map(|t| t.title()).collect();

        assert_eq!(titles.len(), Tab::every().len());

        for (i, title) in titles.iter().enumerate() {
            assert!(!title.is_empty());
            assert!(!titles[i + 1..].contains(title), "{title} appears twice");
        }
    }

    #[test]
    fn the_status_tab_is_the_one_that_opens() {
        assert_eq!(Widgets::default().tab, Tab::Status);
    }

    #[test]
    fn showing_a_tab_selects_it() {
        let mut w = Widgets::default();

        w.show(Tab::Maps);

        assert_eq!(w.tab, Tab::Maps);
    }

    #[test]
    fn a_widget_can_be_turned_off_and_back_on() {
        let mut w = Widgets::default();

        w.disable_widget(Tab::Notes);

        assert!(!w.is_enabled(Tab::Notes));
        assert!(!w.enabled().contains(&Tab::Notes));

        w.enable_widget(Tab::Notes);

        assert!(w.is_enabled(Tab::Notes));
    }

    #[test]
    fn turning_off_the_tab_you_are_on_sends_you_back_to_status() {
        let mut w = Widgets::default();

        w.show(Tab::Maps);
        w.disable_widget(Tab::Maps);

        assert_eq!(w.tab, Tab::Status);
    }

    #[test]
    fn status_cannot_be_turned_off_because_there_would_be_nothing_left() {
        let mut w = Widgets::default();

        w.disable_widget(Tab::Status);

        assert!(w.is_enabled(Tab::Status));
    }

    #[test]
    fn a_disabled_tab_cannot_be_opened() {
        let mut w = Widgets::default();

        w.disable_widget(Tab::Notes);
        w.show(Tab::Notes);

        assert_eq!(w.tab, Tab::Status);
    }

    #[test]
    fn a_widget_is_found_by_the_name_it_shows() {
        let w = Widgets::default();

        assert_eq!(w.find_widget("Maps"), Some(Tab::Maps));
        assert_eq!(w.find_widget("Nothing"), None);
    }

    #[test]
    fn a_priced_item_reaches_the_library() {
        let mut w = Widgets::default();

        w.record(logged("Sapphire Ring"));

        assert_eq!(w.logged(), vec!["Sapphire Ring, 3 chaos".to_string()]);
    }

    #[test]
    fn the_note_line_says_whether_there_is_unsaved_work() {
        let mut w = Widgets::default();

        w.notepad.write("one\ntwo");

        assert!(w.note_line().contains("2 lines"));
        assert!(w.note_line().contains("unsaved"));

        w.notepad.saved();

        assert!(!w.note_line().contains("unsaved"));
    }

    #[test]
    fn the_log_view_keeps_only_the_recent_lines() {
        let mut w = Widgets::default();

        for i in 0..(LOG_LINES_KEPT + 20) {
            w.note_log(&format!("line {i}"));
        }

        assert_eq!(w.log_lines.len(), LOG_LINES_KEPT);
        assert_eq!(
            w.log_lines.last().unwrap(),
            &format!("line {}", LOG_LINES_KEPT + 19),
            "the newest line survives"
        );
    }

    #[test]
    fn a_blank_log_line_is_not_kept() {
        let mut w = Widgets::default();

        w.note_log("   ");

        assert!(w.log_lines.is_empty());
    }

    #[test]
    fn a_search_needs_something_to_search_for() {
        let w = Widgets::default();

        assert!(w.hits(&["Sapphire Ring".to_string()]).is_empty());
    }

    #[test]
    fn a_search_finds_what_was_typed() {
        let w = Widgets {
            needle: "sapph".to_string(),
            ..Widgets::default()
        };

        assert_eq!(w.hits(&["Sapphire Ring".to_string()]).len(), 1);
    }

    #[test]
    fn a_map_with_nothing_marked_is_not_reported_as_dangerous() {
        let mut w = Widgets::default();

        w.check_map(&ParsedItem::default(), 1);

        assert_eq!(w.map_verdict(), Verdict::Unset);
        assert!(w.map_headline().contains('0'));
    }

    #[test]
    fn the_help_list_lines_up_on_the_widest_key() {
        let w = Widgets::default();

        let (listed, width) = w.help(&[
            ("Ctrl+D".to_string(), "price check".to_string()),
            ("Shift+Space".to_string(), "grab the panel".to_string()),
        ]);

        assert_eq!(listed.len(), 2);
        assert_eq!(width, "Shift+Space".chars().count());
    }

    #[test]
    fn the_library_totals_each_currency_it_has_seen() {
        let mut w = Widgets::default();

        w.record(logged("a"));
        w.record(logged("b"));

        assert_eq!(w.totals(), vec!["6 chaos".to_string()]);
    }

    #[test]
    fn a_library_with_nothing_in_it_has_no_totals() {
        assert!(Widgets::default().totals().is_empty());
    }

    #[test]
    fn notes_can_be_opened_from_what_was_saved() {
        let mut w = Widgets::default();

        w.open_notes("chaos recipe");

        assert_eq!(w.notepad.text(), "chaos recipe");
        assert!(!w.notepad.is_dirty(), "opening is not an edit");
    }

    #[test]
    fn clicking_a_map_mod_cycles_its_verdict_and_reports_what_to_save() {
        let mut w = Widgets {
            concerns: vec![Concern {
                text: "monsters deal extra fire damage".to_string(),
                verdict: Verdict::Unset,
            }],
            ..Widgets::default()
        };

        let (matcher, set) = w.cycle_verdict(0).expect("a concern to cycle");

        assert_eq!(matcher, "monsters deal extra fire damage");
        assert_eq!(w.concerns[0].verdict, Verdict::Deadly);
        assert!(set.starts_with('d'), "{set}");
        assert_eq!(w.marked(), 1);
    }

    #[test]
    fn cycling_a_mod_that_is_not_there_changes_nothing() {
        let mut w = Widgets::default();

        assert!(w.cycle_verdict(4).is_none());
    }

    #[test]
    fn a_mod_marked_seen_is_no_longer_counted_as_marked() {
        let mut w = Widgets {
            concerns: vec![Concern {
                text: "x".to_string(),
                verdict: Verdict::Good,
            }],
            ..Widgets::default()
        };

        w.cycle_verdict(0);

        assert_eq!(w.concerns[0].verdict, Verdict::Seen);
        assert_eq!(w.marked(), 0);
    }

    #[test]
    fn the_timer_can_be_put_back_to_zero() {
        let mut w = Widgets::default();

        w.stopwatch.start(0);
        w.stopwatch.stop(5_000);
        w.reset_timer();

        assert_eq!(w.elapsed(9_000), "00:00");
    }

    #[test]
    fn a_verdict_marked_before_is_kept_and_cycled_from_there() {
        let mut w = Widgets {
            concerns: vec![Concern {
                text: "x".to_string(),
                verdict: Verdict::Deadly,
            }],
            ..Widgets::default()
        };

        w.remember_verdicts(vec![("x".to_string(), "d--".to_string())]);

        let (_, set) = w.cycle_verdict(0).expect("cycled");

        assert!(set.starts_with('w'), "deadly cycles to warning: {set}");
        assert_eq!(w.verdicts.len(), 1, "the same mod is not stored twice");
    }

    #[test]
    fn a_remembered_verdict_colours_the_next_map_that_has_that_mod() {
        let mut w = Widgets::default();

        w.remember_verdicts(vec![("x".to_string(), "d--".to_string())]);
        w.check_map(&ParsedItem::default(), 1);

        assert!(
            w.concerns.is_empty(),
            "an item with no mods has no concerns"
        );
    }

    #[test]
    fn entering_a_zone_reaches_the_log_but_a_hideout_does_not() {
        let mut w = Widgets::default();

        w.note_happening(&Happening::EnteredArea {
            name: "Clearfell".into(),
        });
        w.note_happening(&Happening::EnteredArea {
            name: "Felled Hideout".into(),
        });

        assert_eq!(w.log_lines.len(), 1);
        assert!(w.log_lines[0].contains("Clearfell"));
        assert_eq!(w.background.where_you_are(), "Felled Hideout");
    }

    #[test]
    fn the_levelling_step_follows_the_character_level_from_the_log() {
        let mut w = Widgets::default();

        assert!(w.levelling_step().is_none(), "no character seen yet");

        w.note_happening(&Happening::LevelledUp {
            character: "Zelina".into(),
            level: 30,
        });

        assert_eq!(w.levelling_step().expect("a step").act, 3);
        assert_eq!(w.levelling_next().len(), 3);
    }

    #[test]
    fn the_stopwatch_reads_through_the_widgets() {
        let mut w = Widgets::default();

        w.stopwatch.start(0);

        assert_eq!(w.elapsed(65_000), "01:05");
    }
}
