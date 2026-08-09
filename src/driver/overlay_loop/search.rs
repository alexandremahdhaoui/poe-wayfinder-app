use poe_trader_core::types::GameVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOutcome {
    pub total: u64,
    pub id: String,
    pub exchange: bool,
    pub ids: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(id: &str, exchange: bool) -> SearchOutcome {
        SearchOutcome {
            total: 12,
            id: id.to_string(),
            exchange,
            ids: Vec::new(),
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
