use poe_trader_core::types::GameVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Endpoint {
    Search,
    Fetch,
    Exchange,
}

impl Endpoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Endpoint::Search => "search",
            Endpoint::Fetch => "fetch",
            Endpoint::Exchange => "exchange",
        }
    }
}

pub const FETCH_BATCH_SIZE: usize = 10;

#[derive(Debug, Clone)]
pub struct TradeUrls {
    base: String,
    game: GameVersion,
}

impl TradeUrls {
    pub fn new(base_url: &str, game: GameVersion) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            game,
        }
    }

    fn root(&self) -> String {
        format!("{}/api/{}", self.base, self.game.trade_path())
    }

    pub fn search(&self, league: &str) -> String {
        format!("{}/search/{}", self.root(), encode(league))
    }

    pub fn fetch(&self, ids: &[String], query_id: &str) -> String {
        format!(
            "{}/fetch/{}?query={}",
            self.root(),
            ids.join(","),
            encode(query_id)
        )
    }

    pub fn exchange(&self, league: &str) -> String {
        format!("{}/exchange/{}", self.root(), encode(league))
    }

    pub fn data(&self, table: &str) -> String {
        format!("{}/data/{}", self.root(), table)
    }
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }

    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeApiError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for TradeApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "trade api error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for TradeApiError {}

pub fn error_in_body(body: &str) -> Option<TradeApiError> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;

    if error.is_null() {
        return None;
    }

    Some(TradeApiError {
        code: error
            .get("code")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0),
        message: error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no message")
            .to_string(),
    })
}

pub fn fetch_batches(ids: &[String]) -> Vec<Vec<String>> {
    ids.chunks(FETCH_BATCH_SIZE)
        .map(<[String]>::to_vec)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(game: GameVersion) -> TradeUrls {
        TradeUrls::new("https://www.pathofexile.com", game)
    }

    #[test]
    fn poe1_and_poe2_use_different_api_roots() {
        assert_eq!(
            urls(GameVersion::Poe1).search("Standard"),
            "https://www.pathofexile.com/api/trade/search/Standard"
        );
        assert_eq!(
            urls(GameVersion::Poe2).search("Standard"),
            "https://www.pathofexile.com/api/trade2/search/Standard"
        );
    }

    #[test]
    fn a_league_name_with_a_space_is_encoded() {
        assert_eq!(
            urls(GameVersion::Poe2).search("Hardcore Ruthless"),
            "https://www.pathofexile.com/api/trade2/search/Hardcore%20Ruthless"
        );
    }

    #[test]
    fn unreserved_characters_are_left_alone() {
        assert_eq!(encode("abcXYZ019-_.~"), "abcXYZ019-_.~");
    }

    #[test]
    fn reserved_characters_are_escaped() {
        assert_eq!(encode("a/b?c#d&e=f"), "a%2Fb%3Fc%23d%26e%3Df");
    }

    #[test]
    fn non_ascii_is_encoded_byte_by_byte() {
        assert_eq!(encode("é"), "%C3%A9");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_does_not_double_up() {
        let u = TradeUrls::new("https://www.pathofexile.com/", GameVersion::Poe2);

        assert_eq!(
            u.search("Standard"),
            "https://www.pathofexile.com/api/trade2/search/Standard"
        );
    }

    #[test]
    fn a_fetch_url_joins_ids_with_commas_and_carries_the_query_id() {
        let ids = vec!["a1".to_string(), "b2".to_string()];

        assert_eq!(
            urls(GameVersion::Poe2).fetch(&ids, "XyZ123"),
            "https://www.pathofexile.com/api/trade2/fetch/a1,b2?query=XyZ123"
        );
    }

    #[test]
    fn the_exchange_and_data_urls_follow_the_same_root() {
        let u = urls(GameVersion::Poe1);

        assert_eq!(
            u.exchange("Standard"),
            "https://www.pathofexile.com/api/trade/exchange/Standard"
        );
        assert_eq!(
            u.data("stats"),
            "https://www.pathofexile.com/api/trade/data/stats"
        );
    }

    #[test]
    fn a_configured_base_url_is_honoured() {
        let u = TradeUrls::new("http://localhost:8080", GameVersion::Poe2);

        assert_eq!(
            u.search("Standard"),
            "http://localhost:8080/api/trade2/search/Standard"
        );
    }

    #[test]
    fn every_endpoint_has_a_distinct_name() {
        let names: Vec<&str> = [Endpoint::Search, Endpoint::Fetch, Endpoint::Exchange]
            .iter()
            .map(|e| e.as_str())
            .collect();

        assert_eq!(names, vec!["search", "fetch", "exchange"]);
    }

    #[test]
    fn an_error_body_is_detected() {
        let body = r#"{"error":{"code":2,"message":"Query is malformed"}}"#;

        assert_eq!(
            error_in_body(body),
            Some(TradeApiError {
                code: 2,
                message: "Query is malformed".into()
            })
        );
    }

    #[test]
    fn a_success_body_carries_no_error() {
        assert_eq!(error_in_body(r#"{"id":"abc","result":[]}"#), None);
    }

    #[test]
    fn an_explicit_null_error_is_not_an_error() {
        assert_eq!(error_in_body(r#"{"error":null,"result":[]}"#), None);
    }

    #[test]
    fn a_body_that_is_not_json_carries_no_error() {
        assert_eq!(error_in_body("<html>503</html>"), None);
    }

    #[test]
    fn an_error_with_missing_fields_still_reports_something() {
        let got = error_in_body(r#"{"error":{}}"#).unwrap();

        assert_eq!(got.code, 0);
        assert_eq!(got.message, "no message");
    }

    #[test]
    fn an_error_renders_both_its_code_and_its_message() {
        let e = TradeApiError {
            code: 3,
            message: "Rate limit exceeded".into(),
        };

        let rendered = e.to_string();

        assert!(rendered.contains('3'));
        assert!(rendered.contains("Rate limit exceeded"));
    }

    #[test]
    fn a_short_id_list_is_one_batch() {
        let ids: Vec<String> = (0..5).map(|i| i.to_string()).collect();

        assert_eq!(fetch_batches(&ids).len(), 1);
    }

    #[test]
    fn a_long_id_list_is_split_at_the_api_maximum() {
        let ids: Vec<String> = (0..25).map(|i| i.to_string()).collect();

        let batches = fetch_batches(&ids);

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].len(), 10);
        assert_eq!(batches[2].len(), 5);
    }

    #[test]
    fn an_exactly_full_batch_is_not_split() {
        let ids: Vec<String> = (0..10).map(|i| i.to_string()).collect();

        assert_eq!(fetch_batches(&ids).len(), 1);
    }

    #[test]
    fn an_empty_id_list_yields_no_batches() {
        assert!(fetch_batches(&[]).is_empty());
    }
}
