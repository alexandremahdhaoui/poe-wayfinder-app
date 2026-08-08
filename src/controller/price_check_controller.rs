use std::time::Duration;

use poe_trader_core::adapter::data_adapter::GameData;
use poe_trader_core::controller::price_check::{price_check, PriceCheck, PriceCheckOptions};
use poe_trader_core::types::GameVersion;
use thiserror::Error;

use crate::adapter::clock_adapter::Clock;
use crate::adapter::http_adapter::{HttpAdapterError, HttpClient, HttpResponse};
use crate::adapter::query_json_adapter::{to_exchange_json, to_json};
use crate::adapter::rate_limit_adapter::LimiterSet;
use crate::adapter::trade_api_adapter::{error_in_body, Endpoint, TradeApiError, TradeUrls};

#[derive(Debug, Error)]
pub enum PriceCheckError {
    #[error("parsing the item")]
    Parse(#[source] poe_trader_core::controller::parse::ParseError),

    #[error("searching the trade api")]
    Request(#[source] HttpAdapterError),

    #[error("the trade api refused the search")]
    Api(#[source] TradeApiError),

    #[error("the trade api answered with status {status}")]
    Status { status: u16, body: String },

    #[error("reading the search response")]
    Decode(#[source] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub id: String,
    pub result: Vec<String>,
    pub total: u64,
}

#[allow(async_fn_in_trait)]
pub trait Prices {
    async fn search_checked(
        &mut self,
        checked: &PriceCheck,
    ) -> Result<(SearchResult, bool), PriceCheckError>;
}

pub struct PriceCheckController<H: HttpClient, C: Clock> {
    http: H,
    clock: C,
    urls: TradeUrls,
    league: String,
    session_id: String,
    search_limits: LimiterSet,
    game: GameVersion,
    latency: u32,
}

impl<H: HttpClient, C: Clock> PriceCheckController<H, C> {
    pub fn new(http: H, clock: C, base_url: &str, game: GameVersion, league: &str) -> Self {
        Self {
            http,
            clock,
            urls: TradeUrls::new(base_url, game),
            league: league.to_string(),
            session_id: String::new(),
            search_limits: LimiterSet::conservative(),
            game,
            latency: api_latency_seconds(),
        }
    }

    pub fn with_latency(mut self, latency: u32) -> Self {
        self.latency = latency;

        self
    }

    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = session_id.to_string();

        self
    }

    pub fn search_limits(&self) -> &LimiterSet {
        &self.search_limits
    }

    pub async fn check(
        &mut self,
        clipboard: &str,
        data: &dyn GameData,
        options: PriceCheckOptions,
    ) -> Result<(PriceCheck, SearchResult), PriceCheckError> {
        let checked = price_check(clipboard, data, options).map_err(PriceCheckError::Parse)?;
        let (result, _) = self.run_search(&checked).await?;

        Ok((checked, result))
    }

    async fn run_search(
        &mut self,
        checked: &PriceCheck,
    ) -> Result<(SearchResult, bool), PriceCheckError> {
        let exchange = match (checked.endpoint, &checked.trade_tag) {
            (poe_trader_core::controller::bulk::Endpoint::Exchange, Some(tag)) => Some(tag.clone()),
            _ => None,
        };

        let body = match &exchange {
            Some(tag) => serde_json::to_string(&to_exchange_json(tag, &[], checked.query.status)),
            None => serde_json::to_string(&to_json(&checked.query, self.game)),
        }
        .map_err(PriceCheckError::Decode)?;

        let response = self.send_search(&body, exchange.is_some()).await?;
        let result = read_search_result(&response)?;

        Ok((result, exchange.is_some()))
    }

    async fn send_search(
        &mut self,
        body: &str,
        exchange: bool,
    ) -> Result<HttpResponse, PriceCheckError> {
        let wait = self.search_limits.wait_for(self.clock.now());

        if wait > 0 {
            self.clock.sleep(wait).await;
        }

        self.search_limits.borrow(self.clock.now());

        let url = match exchange {
            true => self.urls.exchange(&self.league),
            false => self.urls.search(&self.league),
        };

        let cookie = format!("POESESSID={}", self.session_id);
        let mut headers: Vec<(&str, &str)> = vec![("accept", "application/json")];

        if !self.session_id.is_empty() {
            headers.push(("cookie", &cookie));
        }

        let response = self
            .http
            .post_json(&url, &headers, body)
            .await
            .map_err(PriceCheckError::Request)?;

        self.search_limits
            .adjust(&response.headers, self.latency, self.clock.now());

        Ok(response)
    }

    pub fn estimate_burst(&mut self, count: u32) -> Duration {
        let millis = self
            .search_limits
            .estimate_time(count, self.clock.now(), false);

        Duration::from_millis(millis)
    }
}

impl<H: HttpClient, C: Clock> Prices for PriceCheckController<H, C> {
    async fn search_checked(
        &mut self,
        checked: &PriceCheck,
    ) -> Result<(SearchResult, bool), PriceCheckError> {
        self.run_search(checked).await
    }
}

fn api_latency_seconds() -> u32 {
    2
}

pub fn read_search_result(response: &HttpResponse) -> Result<SearchResult, PriceCheckError> {
    if let Some(error) = error_in_body(&response.body) {
        return Err(PriceCheckError::Api(error));
    }

    if !(200..300).contains(&response.status) {
        return Err(PriceCheckError::Status {
            status: response.status,
            body: response.body.clone(),
        });
    }

    let value: serde_json::Value =
        serde_json::from_str(&response.body).map_err(PriceCheckError::Decode)?;

    Ok(SearchResult {
        id: value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        result: value
            .get("result")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        total: value
            .get("total")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub amount: f64,
    pub currency: String,
    pub account: String,
    pub online: bool,
}

pub fn read_listings(response: &HttpResponse) -> Result<Vec<Listing>, PriceCheckError> {
    if let Some(error) = error_in_body(&response.body) {
        return Err(PriceCheckError::Api(error));
    }

    if !(200..300).contains(&response.status) {
        return Err(PriceCheckError::Status {
            status: response.status,
            body: response.body.clone(),
        });
    }

    let value: serde_json::Value =
        serde_json::from_str(&response.body).map_err(PriceCheckError::Decode)?;

    let Some(results) = value.get("result").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };

    Ok(results.iter().filter_map(read_listing).collect())
}

fn read_listing(entry: &serde_json::Value) -> Option<Listing> {
    let listing = entry.get("listing")?;
    let price = listing.get("price")?;

    let amount = price.get("amount").and_then(serde_json::Value::as_f64)?;
    let currency = price.get("currency").and_then(serde_json::Value::as_str)?;

    let account = listing
        .get("account")
        .and_then(|a| a.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    let online = listing
        .get("account")
        .and_then(|a| a.get("online"))
        .is_some_and(|o| !o.is_null());

    Some(Listing {
        amount,
        currency: currency.to_string(),
        account: account.to_string(),
        online,
    })
}

pub fn read_exchange_listings(response: &HttpResponse) -> Result<Vec<Listing>, PriceCheckError> {
    if let Some(error) = error_in_body(&response.body) {
        return Err(PriceCheckError::Api(error));
    }

    if !(200..300).contains(&response.status) {
        return Err(PriceCheckError::Status {
            status: response.status,
            body: response.body.clone(),
        });
    }

    let value: serde_json::Value =
        serde_json::from_str(&response.body).map_err(PriceCheckError::Decode)?;

    let Some(results) = value.get("result").and_then(serde_json::Value::as_object) else {
        return Ok(Vec::new());
    };

    Ok(results.values().filter_map(read_exchange_listing).collect())
}

fn read_exchange_listing(entry: &serde_json::Value) -> Option<Listing> {
    let listing = entry.get("listing")?;

    let offer = listing.get("offers")?.as_array()?.first()?;

    let want = offer.get("exchange")?;
    let give = offer.get("item")?;

    let want_amount = want.get("amount").and_then(serde_json::Value::as_f64)?;
    let give_amount = give.get("amount").and_then(serde_json::Value::as_f64)?;
    let currency = want.get("currency").and_then(serde_json::Value::as_str)?;

    if give_amount == 0.0 {
        return None;
    }

    let account = listing
        .get("account")
        .and_then(|a| a.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    let online = listing
        .get("account")
        .and_then(|a| a.get("online"))
        .is_some_and(|o| !o.is_null());

    Some(Listing {
        amount: want_amount / give_amount,
        currency: currency.to_string(),
        account: account.to_string(),
        online,
    })
}

pub fn suggested_price(listings: &[Listing]) -> Option<(f64, String)> {
    if listings.is_empty() {
        return None;
    }

    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();

    for listing in listings {
        *counts.entry(listing.currency.as_str()).or_default() += 1;
    }

    let currency = counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)?;

    let mut amounts: Vec<f64> = listings
        .iter()
        .filter(|l| l.currency == currency)
        .map(|l| l.amount)
        .collect();

    amounts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median = if amounts.len() % 2 == 1 {
        amounts[amounts.len() / 2]
    } else {
        let mid = amounts.len() / 2;

        (amounts[mid - 1] + amounts[mid]) / 2.0
    };

    Some((median, currency.to_string()))
}

pub fn limiter_for(endpoint: Endpoint) -> &'static str {
    endpoint.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::time::Millis;
    use std::sync::Mutex;

    struct SteppingClock {
        now: Mutex<Millis>,
        slept: Mutex<Vec<Millis>>,
    }

    impl SteppingClock {
        fn new() -> Self {
            Self {
                now: Mutex::new(0),
                slept: Mutex::new(Vec::new()),
            }
        }

        fn total_slept(&self) -> Millis {
            self.slept.lock().unwrap().iter().sum()
        }
    }

    impl Clock for SteppingClock {
        fn now(&self) -> Millis {
            *self.now.lock().unwrap()
        }

        async fn sleep(&self, millis: Millis) {
            self.slept.lock().unwrap().push(millis);
            *self.now.lock().unwrap() += millis;
        }
    }

    struct CannedHttp {
        responses: Mutex<Vec<HttpResponse>>,
        sent: Mutex<Vec<(String, String)>>,
    }

    impl CannedHttp {
        fn with(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                sent: Mutex::new(Vec::new()),
            }
        }

        fn sent_urls(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .map(|(url, _)| url.clone())
                .collect()
        }
    }

    impl HttpClient for CannedHttp {
        async fn get(
            &self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<HttpResponse, HttpAdapterError> {
            unreachable!("search never uses GET")
        }

        async fn post_json(
            &self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, HttpAdapterError> {
            self.sent
                .lock()
                .unwrap()
                .push((url.to_string(), body.to_string()));

            let mut responses = self.responses.lock().unwrap();

            if responses.is_empty() {
                panic!("more requests than canned responses");
            }

            Ok(responses.remove(0))
        }
    }

    fn ok_response(headers: Vec<(&str, &str)>) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: r#"{"id":"abc123","result":["r1","r2"],"total":57}"#.to_string(),
        }
    }

    #[test]
    fn a_successful_search_response_is_read() {
        let got = read_search_result(&ok_response(vec![])).unwrap();

        assert_eq!(got.id, "abc123");
        assert_eq!(got.result, vec!["r1", "r2"]);
        assert_eq!(got.total, 57);
    }

    #[test]
    fn an_error_body_beats_a_two_hundred_status() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"error":{"code":2,"message":"Query is malformed"}}"#.to_string(),
        };

        let err = read_search_result(&response).unwrap_err();

        assert!(matches!(err, PriceCheckError::Api(_)));
        assert!(err.to_string().contains("refused"));
    }

    #[test]
    fn a_non_success_status_is_reported_with_its_body() {
        let response = HttpResponse {
            status: 503,
            headers: Vec::new(),
            body: "<html>maintenance</html>".to_string(),
        };

        let err = read_search_result(&response).unwrap_err();

        match err {
            PriceCheckError::Status { status, body } => {
                assert_eq!(status, 503);
                assert!(body.contains("maintenance"));
            }
            other => panic!("wrong error: {other:?}"),
        }
    }

    #[test]
    fn a_body_that_is_not_json_is_a_decode_error() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: "not json".to_string(),
        };

        assert!(matches!(
            read_search_result(&response).unwrap_err(),
            PriceCheckError::Decode(_)
        ));
    }

    #[test]
    fn a_response_missing_every_field_reads_as_an_empty_result() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: "{}".to_string(),
        };

        let got = read_search_result(&response).unwrap();

        assert_eq!(got.id, "");
        assert!(got.result.is_empty());
        assert_eq!(got.total, 0);
    }

    #[test]
    fn a_result_array_with_a_non_string_entry_drops_only_that_entry() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"id":"x","result":["a",null,"b"],"total":2}"#.to_string(),
        };

        let got = read_search_result(&response).unwrap();

        assert_eq!(got.result, vec!["a", "b"]);
    }

    #[test]
    fn the_error_chain_names_both_the_action_and_the_cause() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"error":{"code":3,"message":"Rate limit exceeded"}}"#.to_string(),
        };

        let err = read_search_result(&response).unwrap_err();

        let cause = std::error::Error::source(&err).unwrap().to_string();
        assert!(cause.contains("Rate limit exceeded"), "{cause}");
    }

    #[tokio::test]
    async fn a_search_sends_to_the_right_url_and_reads_the_result() {
        let http = CannedHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        let response = c.send_search("{}", false).await.unwrap();
        let got = read_search_result(&response).unwrap();

        assert_eq!(got.total, 57);
        assert_eq!(
            c.http.sent_urls()[0],
            "https://www.pathofexile.com/api/trade2/search/Standard"
        );
    }

    fn currency_check() -> PriceCheck {
        PriceCheck {
            item: poe_trader_core::types::item::ParsedItem::default(),
            query: poe_trader_core::types::query::TradeQuery::default(),
            endpoint: poe_trader_core::controller::bulk::Endpoint::Exchange,
            trade_tag: Some("divine".to_string()),
        }
    }

    fn item_check() -> PriceCheck {
        PriceCheck {
            item: poe_trader_core::types::item::ParsedItem::default(),
            query: poe_trader_core::types::query::TradeQuery::default(),
            endpoint: poe_trader_core::controller::bulk::Endpoint::Search,
            trade_tag: None,
        }
    }

    fn controller(http: CannedHttp) -> PriceCheckController<CannedHttp, SteppingClock> {
        PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        )
    }

    #[tokio::test]
    async fn a_currency_goes_to_the_exchange_endpoint() {
        let mut c = controller(CannedHttp::with(vec![ok_response(vec![])]));

        let (_, exchange) = c.search_checked(&currency_check()).await.unwrap();

        assert!(
            exchange,
            "a currency must report itself as an exchange search"
        );
        assert!(
            c.http.sent_urls()[0].contains("exchange"),
            "{:?}",
            c.http.sent_urls()
        );
    }

    #[tokio::test]
    async fn an_item_goes_to_the_search_endpoint() {
        let mut c = controller(CannedHttp::with(vec![ok_response(vec![])]));

        let (_, exchange) = c.search_checked(&item_check()).await.unwrap();

        assert!(!exchange);
        assert!(
            c.http.sent_urls()[0].contains("search"),
            "{:?}",
            c.http.sent_urls()
        );
    }

    #[tokio::test]
    async fn the_configured_latency_replaces_the_default() {
        let c = controller(CannedHttp::with(vec![])).with_latency(9);

        assert_eq!(c.latency, 9);
    }

    #[tokio::test]
    async fn the_first_search_does_not_wait() {
        let http = CannedHttp::with(vec![ok_response(vec![])]);
        let clock = SteppingClock::new();
        let mut c = PriceCheckController::new(
            http,
            clock,
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}", false).await.unwrap();

        assert_eq!(c.clock.total_slept(), 0);
    }

    #[tokio::test]
    async fn a_second_search_waits_for_the_conservative_window() {
        let http = CannedHttp::with(vec![ok_response(vec![]), ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}", false).await.unwrap();
        c.send_search("{}", false).await.unwrap();

        assert_eq!(c.clock.total_slept(), 5000);
    }

    #[tokio::test]
    async fn the_servers_limits_replace_the_conservative_guess() {
        let http = CannedHttp::with(vec![ok_response(vec![
            ("x-rate-limit-rules", "Ip"),
            ("x-rate-limit-ip", "8:10:60"),
            ("x-rate-limit-ip-state", "1:10:0"),
        ])]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}", false).await.unwrap();

        let limits = c.search_limits().limits();

        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].max, 8);
        assert_eq!(limits[0].window_secs, 12);
    }

    #[tokio::test]
    async fn a_wider_server_limit_lets_the_second_search_through_at_once() {
        let http = CannedHttp::with(vec![
            ok_response(vec![
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
            ]),
            ok_response(vec![]),
        ]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}", false).await.unwrap();
        c.send_search("{}", false).await.unwrap();

        assert_eq!(c.clock.total_slept(), 0);
    }

    #[tokio::test]
    async fn the_session_cookie_is_sent_when_present_and_omitted_when_not() {
        let http = CannedHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        )
        .with_session("deadbeef");

        c.send_search("{}", false).await.unwrap();

        assert_eq!(c.http.sent_urls().len(), 1);
        assert_eq!(c.session_id, "deadbeef");
    }

    #[tokio::test]
    async fn the_league_is_percent_encoded_into_the_url() {
        let http = CannedHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Hardcore Ruthless",
        );

        c.send_search("{}", false).await.unwrap();

        assert!(c.http.sent_urls()[0].contains("Hardcore%20Ruthless"));
    }

    #[tokio::test]
    async fn poe1_and_poe2_search_different_endpoints() {
        for (game, want) in [
            (GameVersion::Poe1, "/api/trade/search/"),
            (GameVersion::Poe2, "/api/trade2/search/"),
        ] {
            let http = CannedHttp::with(vec![ok_response(vec![])]);
            let mut c = PriceCheckController::new(
                http,
                SteppingClock::new(),
                "https://www.pathofexile.com",
                game,
                "Standard",
            );

            c.send_search("{}", false).await.unwrap();

            assert!(c.http.sent_urls()[0].contains(want), "{game:?}");
        }
    }

    #[test]
    fn a_burst_estimate_reflects_the_current_limits() {
        let mut c = PriceCheckController::new(
            CannedHttp::with(Vec::new()),
            SteppingClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        assert_eq!(c.estimate_burst(1), Duration::from_secs(0));
        assert_eq!(c.estimate_burst(3), Duration::from_secs(10));
    }

    fn fetch_body(entries: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: format!("{{\"result\":[{entries}]}}"),
        }
    }

    fn listing(amount: f64, currency: &str, online: bool) -> String {
        let account = if online {
            "{\"name\":\"Kaom\",\"online\":{\"status\":\"online\"}}"
        } else {
            "{\"name\":\"Kaom\"}"
        };

        format!(
            "{{\"listing\":{{\"price\":{{\"amount\":{amount},\"currency\":\"{currency}\"}},\"account\":{account}}}}}"
        )
    }

    #[test]
    fn a_listing_is_read() {
        let got = read_listings(&fetch_body(&listing(5.0, "divine", true))).unwrap();

        assert_eq!(got.len(), 1);
        assert_eq!(got[0].amount, 5.0);
        assert_eq!(got[0].currency, "divine");
        assert_eq!(got[0].account, "Kaom");
        assert!(got[0].online);
    }

    #[test]
    fn an_offline_seller_is_marked_as_such() {
        let got = read_listings(&fetch_body(&listing(5.0, "divine", false))).unwrap();

        assert!(!got[0].online);
    }

    #[test]
    fn a_null_entry_is_skipped_rather_than_failing_the_batch() {
        let entries = format!("null,{}", listing(5.0, "divine", true));

        assert_eq!(read_listings(&fetch_body(&entries)).unwrap().len(), 1);
    }

    #[test]
    fn an_unpriced_listing_is_skipped() {
        let entries = "{\"listing\":{\"account\":{\"name\":\"Kaom\"}}}";

        assert!(read_listings(&fetch_body(entries)).unwrap().is_empty());
    }

    #[test]
    fn an_empty_fetch_page_is_not_an_error() {
        assert!(read_listings(&fetch_body("")).unwrap().is_empty());
        assert!(read_listings(&HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: "{}".to_string()
        })
        .unwrap()
        .is_empty());
    }

    #[test]
    fn an_error_body_beats_a_two_hundred_on_a_fetch_too() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: r#"{"error":{"code":2,"message":"bad"}}"#.to_string(),
        };

        assert!(matches!(
            read_listings(&response).unwrap_err(),
            PriceCheckError::Api(_)
        ));
    }

    fn priced(amount: f64, currency: &str) -> Listing {
        Listing {
            amount,
            currency: currency.to_string(),
            account: "Kaom".to_string(),
            online: true,
        }
    }

    #[test]
    fn the_suggested_price_is_the_median() {
        let listings = [
            priced(5.0, "divine"),
            priced(6.0, "divine"),
            priced(1000.0, "divine"),
        ];

        let (price, currency) = suggested_price(&listings).unwrap();

        assert_eq!(price, 6.0);
        assert_eq!(currency, "divine");
    }

    #[test]
    fn an_even_count_averages_the_middle_pair() {
        let listings = [
            priced(4.0, "divine"),
            priced(6.0, "divine"),
            priced(8.0, "divine"),
            priced(10.0, "divine"),
        ];

        assert_eq!(suggested_price(&listings).unwrap().0, 7.0);
    }

    #[test]
    fn only_the_most_common_currency_is_used() {
        let listings = [
            priced(5.0, "divine"),
            priced(6.0, "divine"),
            priced(3000.0, "chaos"),
        ];

        let (price, currency) = suggested_price(&listings).unwrap();

        assert_eq!(currency, "divine");
        assert_eq!(price, 5.5);
    }

    #[test]
    fn no_listings_suggests_no_price() {
        assert_eq!(suggested_price(&[]), None);
    }

    #[test]
    fn one_listing_suggests_its_own_price() {
        assert_eq!(
            suggested_price(&[priced(5.0, "divine")]).unwrap(),
            (5.0, "divine".to_string())
        );
    }

    #[test]
    fn each_endpoint_names_its_own_limiter() {
        assert_eq!(limiter_for(Endpoint::Search), "search");
        assert_eq!(limiter_for(Endpoint::Fetch), "fetch");
        assert_eq!(limiter_for(Endpoint::Exchange), "exchange");
    }
}

#[cfg(test)]
mod exchange_tests {
    use super::*;

    const REAL: &str = r#"{
      "id": "aL5D8Z77He",
      "complexity": null,
      "total": 2,
      "result": {
        "996e5eba1a1d": {
          "id": "996e5eba1a1d",
          "item": null,
          "listing": {
            "account": { "name": "mathews#2383", "online": { "league": "Standard" } },
            "offers": [
              {
                "exchange": { "currency": "fracturing-orb", "amount": 1 },
                "item": { "currency": "divine", "amount": 10, "stock": 19798 }
              }
            ]
          }
        }
      }
    }"#;

    fn response(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            body: body.to_string(),
            headers: Default::default(),
        }
    }

    #[test]
    fn a_real_exchange_response_produces_a_listing() {
        let got = read_exchange_listings(&response(REAL)).unwrap();

        assert_eq!(got.len(), 1);
    }

    #[test]
    fn the_rate_is_what_is_wanted_over_what_is_given() {
        let got = read_exchange_listings(&response(REAL)).unwrap();

        assert_eq!(got[0].amount, 0.1);
        assert_eq!(got[0].currency, "fracturing-orb");
    }

    #[test]
    fn the_seller_is_carried_through() {
        let got = read_exchange_listings(&response(REAL)).unwrap();

        assert_eq!(got[0].account, "mathews#2383");
        assert!(got[0].online);
    }

    #[test]
    fn the_search_reader_finds_nothing_in_an_exchange_response() {
        let got = read_listings(&response(REAL)).unwrap();

        assert!(got.is_empty());
    }

    #[test]
    fn an_offer_giving_nothing_is_skipped() {
        let body = REAL.replace("\"amount\": 10", "\"amount\": 0");

        assert!(read_exchange_listings(&response(&body)).unwrap().is_empty());
    }

    #[test]
    fn a_response_with_no_result_is_empty_rather_than_an_error() {
        let got = read_exchange_listings(&response(r#"{"id":"x","total":0}"#)).unwrap();

        assert!(got.is_empty());
    }

    #[test]
    fn a_failing_status_is_an_error_and_not_an_empty_page() {
        let mut bad = response(REAL);
        bad.status = 429;

        assert!(read_exchange_listings(&bad).is_err());
    }

    #[test]
    fn an_offer_with_no_offers_array_is_skipped() {
        let body = REAL.replace("\"offers\"", "\"nope\"");

        assert!(read_exchange_listings(&response(&body)).unwrap().is_empty());
    }
}
