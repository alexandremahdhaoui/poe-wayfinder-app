use poe_trader_core::controller::price_check::PriceCheck;

use crate::controller::overlay_controller::OverlayModel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Priced { total: u64 },
    CopyFailed,
    ParseFailed,
    SearchFailed,
    TooBroad,
}

pub fn run<C, P, S>(
    model: &mut OverlayModel,
    cursor: (i32, i32),
    copy: C,
    price: P,
    search: S,
) -> Outcome
where
    C: FnOnce() -> Result<String, String>,
    P: FnOnce(&str) -> Result<PriceCheck, String>,
    S: FnOnce(&PriceCheck) -> Result<u64, String>,
{
    model.start(cursor);

    let text = match copy() {
        Ok(text) => text,
        Err(message) => {
            model.fail(&format!("Could not copy the item: {message}"));

            return Outcome::CopyFailed;
        }
    };

    let checked = match price(&text) {
        Ok(checked) => checked,
        Err(message) => {
            model.fail(&format!("Could not read the item: {message}"));

            return Outcome::ParseFailed;
        }
    };

    if !checked.constrains_something() {
        model.finish(checked, 0);
        model.warn("Nothing to search on. The base type is missing from the data file.");

        return Outcome::TooBroad;
    }

    match search(&checked) {
        Ok(total) => {
            model.finish(checked, total);

            Outcome::Priced { total }
        }
        Err(message) => {
            model.finish(checked, 0);
            model.warn(&message);

            Outcome::SearchFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::overlay::{OverlayGeometry, OverlayState};
    use poe_trader_core::controller::bulk::Endpoint;
    use poe_trader_core::types::item::ParsedItem;
    use poe_trader_core::types::query::{NameField, TradeQuery};

    fn model() -> OverlayModel {
        OverlayModel::new(OverlayGeometry::default())
    }

    fn checked() -> PriceCheck {
        PriceCheck {
            item: ParsedItem::default(),
            query: TradeQuery {
                type_name: Some(NameField::Plain("Sapphire Ring".into())),
                ..TradeQuery::default()
            },
            endpoint: Endpoint::Search,
            trade_tag: None,
            sources: Vec::new(),
        }
    }

    fn unconstrained() -> PriceCheck {
        PriceCheck {
            item: ParsedItem::default(),
            query: TradeQuery::default(),
            endpoint: Endpoint::Search,
            trade_tag: None,
            sources: Vec::new(),
        }
    }

    const ITEM: &str = "Item Class: Rings\nRarity: Rare\nDoom Loop\nSapphire Ring\n";

    #[test]
    fn a_whole_price_check_reaches_the_panel() {
        let mut m = model();

        let got = run(
            &mut m,
            (100, 100),
            || Ok(ITEM.to_string()),
            |text| {
                assert_eq!(text, ITEM);

                Ok(checked())
            },
            |_| Ok(42),
        );

        assert_eq!(got, Outcome::Priced { total: 42 });
        assert_eq!(m.total(), Some(42));
        assert_eq!(m.state(), OverlayState::Showing);
    }

    #[test]
    fn the_panel_opens_before_the_slow_work_starts() {
        let mut m = model();

        run(
            &mut m,
            (100, 100),
            || Err("still busy".to_string()),
            |_| Ok(checked()),
            |_| Ok(1),
        );

        assert_ne!(m.state(), OverlayState::Hidden);
    }

    #[test]
    fn a_failed_copy_says_so_and_stops() {
        let mut m = model();
        let mut priced = false;

        let got = run(
            &mut m,
            (0, 0),
            || Err("clipboard timed out".to_string()),
            |_| {
                priced = true;

                Ok(checked())
            },
            |_| Ok(1),
        );

        assert_eq!(got, Outcome::CopyFailed);
        assert!(!priced, "parsing ran on a failed copy");
        assert!(m
            .message()
            .expect("a message")
            .contains("clipboard timed out"));
    }

    #[test]
    fn a_failed_parse_says_so_and_never_searches() {
        let mut m = model();
        let mut searched = false;

        let got = run(
            &mut m,
            (0, 0),
            || Ok("milk, eggs".to_string()),
            |_| Err("text is not a copied item".to_string()),
            |_| {
                searched = true;

                Ok(1)
            },
        );

        assert_eq!(got, Outcome::ParseFailed);
        assert!(!searched, "searched on an item that did not parse");
        assert!(m
            .message()
            .expect("a message")
            .contains("not a copied item"));
    }

    #[test]
    fn a_failed_search_still_shows_the_item() {
        let mut m = model();

        let got = run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(checked()),
            |_| Err("429 Too Many Requests".to_string()),
        );

        assert_eq!(got, Outcome::SearchFailed);
        assert!(m.result().is_some(), "the parsed item was thrown away");
        assert!(m.message().expect("a message").contains("429"));
    }

    #[test]
    fn a_query_that_narrows_nothing_is_not_sent() {
        let mut m = model();
        let mut searched = false;

        let got = run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(unconstrained()),
            |_| {
                searched = true;

                Ok(1)
            },
        );

        assert_eq!(got, Outcome::TooBroad);
        assert!(!searched, "sent a query that matches everything");
    }

    #[test]
    fn a_query_that_narrows_nothing_names_the_cause() {
        let mut m = model();

        run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(unconstrained()),
            |_| Ok(1),
        );

        assert!(m.message().expect("a message").contains("data file"));
    }

    #[test]
    fn a_query_that_narrows_nothing_still_shows_what_was_read() {
        let mut m = model();

        run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(unconstrained()),
            |_| Ok(1),
        );

        assert!(m.result().is_some());
    }

    #[test]
    fn the_text_copied_is_the_text_parsed() {
        let mut m = model();
        let mut seen = String::new();

        run(
            &mut m,
            (0, 0),
            || Ok("SENTINEL".to_string()),
            |text| {
                seen = text.to_string();

                Ok(checked())
            },
            |_| Ok(1),
        );

        assert_eq!(seen, "SENTINEL");
    }

    #[test]
    fn the_item_parsed_is_the_item_searched() {
        let mut m = model();
        let mut seen: Option<String> = None;

        run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(checked()),
            |c| {
                seen = c.query.type_name.as_ref().map(|n| n.name().to_string());

                Ok(1)
            },
        );

        assert_eq!(seen.as_deref(), Some("Sapphire Ring"));
    }

    #[test]
    fn a_search_returning_no_results_is_still_a_price_check() {
        let mut m = model();

        let got = run(
            &mut m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(checked()),
            |_| Ok(0),
        );

        assert_eq!(got, Outcome::Priced { total: 0 });
        assert_eq!(m.state(), OverlayState::Showing);
        assert_eq!(m.message(), None);
    }

    #[test]
    fn the_panel_opens_where_the_cursor_is() {
        let mut m = model();

        run(
            &mut m,
            (640, 480),
            || Ok(ITEM.to_string()),
            |_| Ok(checked()),
            |_| Ok(1),
        );

        let frame = m.frame(None);

        assert!(frame.rect.is_none() || frame.rect.is_some());
        assert_eq!(m.state(), OverlayState::Showing);
    }

    type FailureCase = (&'static str, fn(&mut OverlayModel) -> Outcome);

    fn failing_copy(m: &mut OverlayModel) -> Outcome {
        run(
            m,
            (0, 0),
            || Err("x".to_string()),
            |_| Ok(checked()),
            |_| Ok(1),
        )
    }

    fn failing_parse(m: &mut OverlayModel) -> Outcome {
        run(
            m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Err("x".to_string()),
            |_| Ok(1),
        )
    }

    fn failing_search(m: &mut OverlayModel) -> Outcome {
        run(
            m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(checked()),
            |_| Err("x".to_string()),
        )
    }

    fn too_broad(m: &mut OverlayModel) -> Outcome {
        run(
            m,
            (0, 0),
            || Ok(ITEM.to_string()),
            |_| Ok(unconstrained()),
            |_| Ok(1),
        )
    }

    #[test]
    fn every_failure_leaves_a_message_the_user_can_read() {
        let cases: [FailureCase; 4] = [
            ("copy", failing_copy),
            ("parse", failing_parse),
            ("search", failing_search),
            ("too broad", too_broad),
        ];

        for (name, case) in cases {
            let mut m = model();
            let got = case(&mut m);

            assert_ne!(got, Outcome::Priced { total: 1 }, "{name}");
            assert!(m.message().is_some(), "{name} failed silently");
        }
    }

    #[test]
    fn a_failure_after_the_parse_keeps_the_item_on_screen() {
        for (name, case) in [
            ("search", failing_search as fn(&mut OverlayModel) -> Outcome),
            ("too broad", too_broad),
        ] {
            let mut m = model();
            case(&mut m);

            assert!(m.result().is_some(), "{name} threw the parsed item away");
        }
    }

    #[test]
    fn a_failure_before_the_parse_shows_nothing_to_act_on() {
        for (name, case) in [
            ("copy", failing_copy as fn(&mut OverlayModel) -> Outcome),
            ("parse", failing_parse),
        ] {
            let mut m = model();
            case(&mut m);

            assert!(m.result().is_none(), "{name} showed an item it never read");
        }
    }
}
