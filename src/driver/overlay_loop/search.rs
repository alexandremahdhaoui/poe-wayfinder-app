use poe_trader_core::types::GameVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    pub total: u64,
    pub id: String,
    pub exchange: bool,
}

impl SearchOutcome {
    pub fn browser_url(&self, base: &str, game: GameVersion, league: &str) -> Option<String> {
        if self.id.is_empty() {
            return None;
        }

        let section = match (game, self.exchange) {
            (GameVersion::Poe1, false) => "trade/search",
            (GameVersion::Poe1, true) => "trade/exchange",
            (GameVersion::Poe2, false) => "trade2/search/poe2",
            (GameVersion::Poe2, true) => "trade2/exchange/poe2",
        };

        Some(format!(
            "{}/{section}/{}/{}",
            base.trim_end_matches('/'),
            urlencode(league),
            self.id
        ))
    }
}

pub fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }

    out
}

#[cfg(windows)]
mod win {
    use super::SearchOutcome;

    use crate::adapter::http_adapter::{HttpAdapter, HttpClient};
    use crate::adapter::query_json_adapter::{to_exchange_json, to_json};
    use crate::adapter::rate_limit_adapter::LimiterSet;
    use crate::adapter::trade_api_adapter::TradeUrls;
    use crate::controller::price_check_controller::read_search_result;
    use crate::util::error_chain::render;

    use poe_trader_core::controller::bulk::Endpoint;
    use poe_trader_core::controller::price_check::PriceCheck;
    use poe_trader_core::types::GameVersion;

    pub struct SearchDeps<'a> {
        pub http: &'a HttpAdapter,
        pub urls: &'a TradeUrls,
        pub league: &'a str,
        pub session: &'a str,
        pub latency: u32,
        pub game: GameVersion,
    }

    pub fn now_millis() -> u64 {
        use std::sync::OnceLock;
        use std::time::Instant;

        static START: OnceLock<Instant> = OnceLock::new();

        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    fn headers_for(session: &str, cookie: &str) -> Vec<(&'static str, String)> {
        match session.is_empty() {
            true => vec![("accept", "application/json".to_string())],
            false => vec![
                ("accept", "application/json".to_string()),
                ("cookie", cookie.to_string()),
            ],
        }
    }

    pub async fn search(
        deps: SearchDeps<'_>,
        limits: &mut LimiterSet,
        checked: &PriceCheck,
    ) -> Result<SearchOutcome, String> {
        if !checked.constrains_something() {
            return Err(
                "Nothing to search on. The base type is missing from the data file. \
                 Rebuild it from the tray."
                    .to_string(),
            );
        }

        let exchange = match (checked.endpoint, &checked.trade_tag) {
            (Endpoint::Exchange, Some(tag)) => Some(tag.clone()),
            _ => None,
        };

        let body = match &exchange {
            Some(tag) => serde_json::to_string(&to_exchange_json(tag, &[], checked.query.status)),
            None => serde_json::to_string(&to_json(&checked.query, deps.game)),
        }
        .map_err(|e| format!("building the search body: {e}"))?;

        let wait = limits.wait_for(now_millis());

        if wait > 0 {
            std::thread::sleep(std::time::Duration::from_millis(wait));
        }

        limits.borrow(now_millis());

        let cookie = format!("POESESSID={}", deps.session);
        let owned = headers_for(deps.session, &cookie);
        let headers: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let url = match &exchange {
            Some(_) => deps.urls.exchange(deps.league),
            None => deps.urls.search(deps.league),
        };

        let response = deps
            .http
            .post_json(&url, &headers, &body)
            .await
            .map_err(|e| render(&e))?;

        limits.adjust(&response.headers, deps.latency, now_millis());

        let result = read_search_result(&response).map_err(|e| render(&e))?;

        Ok(SearchOutcome {
            total: result.total,
            id: result.id,
            exchange: exchange.is_some(),
        })
    }
}

#[cfg(windows)]
pub use win::{now_millis, search, SearchDeps};

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(id: &str, exchange: bool) -> SearchOutcome {
        SearchOutcome {
            total: 12,
            id: id.to_string(),
            exchange,
        }
    }

    const SITE: &str = "https://www.pathofexile.com";

    #[test]
    fn a_poe1_search_opens_the_trade_path() {
        let got = outcome("abc123", false).browser_url(SITE, GameVersion::Poe1, "Standard");

        assert_eq!(
            got,
            Some("https://www.pathofexile.com/trade/search/Standard/abc123".to_string())
        );
    }

    #[test]
    fn a_poe2_search_opens_the_trade2_path() {
        let got = outcome("abc123", false).browser_url(SITE, GameVersion::Poe2, "Standard");

        assert_eq!(
            got,
            Some("https://www.pathofexile.com/trade2/search/poe2/Standard/abc123".to_string())
        );
    }

    #[test]
    fn a_currency_search_opens_the_exchange_path() {
        let got = outcome("abc123", true).browser_url(SITE, GameVersion::Poe2, "Standard");

        assert_eq!(
            got,
            Some("https://www.pathofexile.com/trade2/exchange/poe2/Standard/abc123".to_string())
        );
    }

    #[test]
    fn a_search_with_no_id_opens_nothing() {
        assert_eq!(
            outcome("", false).browser_url(SITE, GameVersion::Poe1, "Standard"),
            None
        );
    }

    #[test]
    fn a_trailing_slash_on_the_site_url_does_not_double() {
        let got = outcome("x", false).browser_url("https://site/", GameVersion::Poe1, "Standard");

        assert_eq!(
            got,
            Some("https://site/trade/search/Standard/x".to_string())
        );
    }

    #[test]
    fn a_league_with_a_space_is_encoded() {
        let got = outcome("x", false).browser_url(SITE, GameVersion::Poe1, "Rise of the Abyssal");

        assert_eq!(
            got,
            Some(
                "https://www.pathofexile.com/trade/search/Rise%20of%20the%20Abyssal/x".to_string()
            )
        );
    }

    #[test]
    fn encoding_leaves_the_unreserved_characters_alone() {
        assert_eq!(urlencode("aZ0-_.~"), "aZ0-_.~");
    }

    #[test]
    fn encoding_escapes_what_a_url_cannot_carry() {
        assert_eq!(urlencode("a b/c?d"), "a%20b%2Fc%3Fd");
    }
}
