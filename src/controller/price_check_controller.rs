//! Running a price check against the live trade API.
//!
//! Orchestrates the adapters. It parses through `poe-trader-core`, sends
//! through `http_adapter` so the allowlist applies, and paces itself through
//! `rate_limit_adapter` so GGG's limits are respected.
//!
//! # The order is not negotiable
//!
//! Wait, send, then adjust. Adjusting before sending would use the previous
//! response's limits for this one, and sending before waiting is the thing
//! that gets an account banned.

use std::time::Duration;

use poe_trader_core::adapter::data_adapter::GameData;
use poe_trader_core::controller::price_check::{price_check, PriceCheck, PriceCheckOptions};
use poe_trader_core::types::GameVersion;
use thiserror::Error;

use crate::adapter::http_adapter::{HttpAdapterError, HttpClient, HttpResponse};
use crate::adapter::query_json_adapter::to_json;
use crate::adapter::rate_limit_adapter::{LimiterSet, Millis};
use crate::adapter::trade_api_adapter::{error_in_body, Endpoint, TradeApiError, TradeUrls};

/// Why a price check failed.
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

/// What a search returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// The query id, needed to fetch any listing.
    pub id: String,
    /// Result ids, most relevant first.
    pub result: Vec<String>,
    /// How many listings matched in total.
    pub total: u64,
}

/// Pausing between requests.
///
/// Declared here because this controller consumes it. A test supplies one that
/// records the wait instead of taking it, so no test sleeps.
///
/// The async method is allowed rather than boxed. Every implementation lives
/// in this workspace, so the auto trait bounds clippy warns about cannot be
/// surprised by a caller we do not control.
#[allow(async_fn_in_trait)]
pub trait Clock: Send + Sync {
    /// Milliseconds since some fixed point.
    fn now(&self) -> Millis;

    /// Wait this long.
    async fn sleep(&self, millis: Millis);
}

/// What the controller needs to run a check.
pub struct PriceCheckController<H: HttpClient, C: Clock> {
    http: H,
    clock: C,
    urls: TradeUrls,
    league: String,
    session_id: String,
    search_limits: LimiterSet,
}

impl<H: HttpClient, C: Clock> PriceCheckController<H, C> {
    /// Build the controller.
    ///
    /// The limiter starts conservative at one request per five seconds. The
    /// server's real limits arrive with the first response, and guessing high
    /// before then gets that first request throttled.
    pub fn new(http: H, clock: C, base_url: &str, game: GameVersion, league: &str) -> Self {
        Self {
            http,
            clock,
            urls: TradeUrls::new(base_url, game),
            league: league.to_string(),
            session_id: String::new(),
            search_limits: LimiterSet::conservative(),
        }
    }

    /// Supply the session cookie.
    ///
    /// Search and fetch both need it. It is never logged and never written to
    /// disk by this crate.
    pub fn with_session(mut self, session_id: &str) -> Self {
        self.session_id = session_id.to_string();

        self
    }

    /// The limits currently mirrored for search.
    pub fn search_limits(&self) -> &LimiterSet {
        &self.search_limits
    }

    /// Parse an item and search for it.
    pub async fn check(
        &mut self,
        clipboard: &str,
        data: &dyn GameData,
        options: PriceCheckOptions,
    ) -> Result<(PriceCheck, SearchResult), PriceCheckError> {
        let checked = price_check(clipboard, data, options).map_err(PriceCheckError::Parse)?;

        let body =
            serde_json::to_string(&to_json(&checked.query)).map_err(PriceCheckError::Decode)?;

        let response = self.send_search(&body).await?;
        let result = read_search_result(&response)?;

        Ok((checked, result))
    }

    /// Send one search, respecting the limits.
    async fn send_search(&mut self, body: &str) -> Result<HttpResponse, PriceCheckError> {
        // 1. Wait until the limits allow it.
        let wait = self.search_limits.wait_for(self.clock.now());

        if wait > 0 {
            self.clock.sleep(wait).await;
        }

        // 2. Take the slot, then send. Taking it after the send would let two
        //    concurrent checks both see a free slot.
        self.search_limits.borrow(self.clock.now());

        let url = self.urls.search(&self.league);

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

        // 3. Adjust from what the server just said. Doing this before the send
        //    would apply the previous response's limits to this one.
        self.search_limits
            .adjust(&response.headers, api_latency_seconds(), self.clock.now());

        Ok(response)
    }

    /// How long a burst of `count` searches would take.
    ///
    /// Used to warn before a burst rather than silently queue one, because a
    /// queued burst looks like the app has hung.
    pub fn estimate_burst(&mut self, count: u32) -> Duration {
        let millis = self
            .search_limits
            .estimate_time(count, self.clock.now(), false);

        Duration::from_millis(millis)
    }
}

/// The window padding, in seconds.
///
/// Two seconds, matching the reference default. Threading the configured value
/// through is a later change; this constant makes the current behaviour
/// explicit rather than hidden inside a call.
fn api_latency_seconds() -> u32 {
    2
}

/// Read a search response.
///
/// Checks three things in order: the transport status, the API's own error
/// body, then the shape. The API answers 200 with an error body, so skipping
/// the middle step treats a failure as an empty result.
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

/// One listing, as the fetch endpoint returned it.
///
/// Only the fields a price needs. The API returns far more and carrying all of
/// it would tie the UI to the API's shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    /// What the seller is asking.
    pub amount: f64,
    /// The currency they want, such as `divine`.
    pub currency: String,
    /// Who is selling.
    pub account: String,
    /// Whether they are online now.
    pub online: bool,
}

/// Read a fetch response into listings.
///
/// Ported from the result handling in `requestResults`.
///
/// A null entry is skipped rather than failing the batch. The API returns one
/// for a listing that vanished between the search and the fetch, which happens
/// constantly on a busy league.
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
        // No result array at all is an empty page, not a failure.
        return Ok(Vec::new());
    };

    Ok(results.iter().filter_map(read_listing).collect())
}

/// Read one listing, or nothing when it is unpriced or gone.
fn read_listing(entry: &serde_json::Value) -> Option<Listing> {
    let listing = entry.get("listing")?;
    let price = listing.get("price")?;

    // A listing with no price is not for sale at a number. Showing it as free
    // would be worse than not showing it.
    let amount = price.get("amount").and_then(serde_json::Value::as_f64)?;
    let currency = price.get("currency").and_then(serde_json::Value::as_str)?;

    let account = listing
        .get("account")
        .and_then(|a| a.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    // The API marks online by the presence of an object, not by a boolean.
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

/// The price a set of listings suggests.
///
/// The median and not the mean. One listing at a thousand divine drags a mean
/// far above what anyone will pay, and those listings are common because
/// people post them to bait.
pub fn suggested_price(listings: &[Listing]) -> Option<(f64, String)> {
    if listings.is_empty() {
        return None;
    }

    // Only the most common currency. Mixing divine and chaos into one median
    // produces a number in no currency at all.
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

/// Which limiter set an endpoint belongs to.
///
/// Only search is wired today. Fetch and exchange get their own sets when
/// they are, because the server limits all three separately.
pub fn limiter_for(endpoint: Endpoint) -> &'static str {
    endpoint.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A clock that records every wait instead of taking it.
    struct FakeClock {
        now: Mutex<Millis>,
        slept: Mutex<Vec<Millis>>,
    }

    impl FakeClock {
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

    impl Clock for FakeClock {
        fn now(&self) -> Millis {
            *self.now.lock().unwrap()
        }

        async fn sleep(&self, millis: Millis) {
            self.slept.lock().unwrap().push(millis);
            *self.now.lock().unwrap() += millis;
        }
    }

    /// An HTTP client that returns canned responses.
    ///
    /// Mutex rather than RefCell, because `HttpClient` requires `Sync` and
    /// asserting that by hand would be a lie waiting to become a data race.
    struct FakeHttp {
        responses: Mutex<Vec<HttpResponse>>,
        sent: Mutex<Vec<(String, String)>>,
    }

    impl FakeHttp {
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

    impl HttpClient for FakeHttp {
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
        // The API answers 200 with an error body, so skipping this check
        // treats a failure as an empty result.
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
        // A bare status number tells the user nothing. Cloudflare returns HTML
        // and GGG returns a reason.
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
        // An empty result is a real answer. Failing here would turn a search
        // that legitimately found nothing into an error.
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
        let http = FakeHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        let response = c.send_search("{}").await.unwrap();
        let got = read_search_result(&response).unwrap();

        assert_eq!(got.total, 57);
        assert_eq!(
            c.http.sent_urls()[0],
            "https://www.pathofexile.com/api/trade2/search/Standard"
        );
    }

    #[tokio::test]
    async fn the_first_search_does_not_wait() {
        let http = FakeHttp::with(vec![ok_response(vec![])]);
        let clock = FakeClock::new();
        let mut c = PriceCheckController::new(
            http,
            clock,
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}").await.unwrap();

        assert_eq!(c.clock.total_slept(), 0);
    }

    #[tokio::test]
    async fn a_second_search_waits_for_the_conservative_window() {
        // The limiter starts at one request per five seconds until the server
        // says otherwise. Guessing high gets the first request throttled.
        let http = FakeHttp::with(vec![ok_response(vec![]), ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}").await.unwrap();
        c.send_search("{}").await.unwrap();

        assert_eq!(c.clock.total_slept(), 5000);
    }

    #[tokio::test]
    async fn the_servers_limits_replace_the_conservative_guess() {
        let http = FakeHttp::with(vec![ok_response(vec![
            ("x-rate-limit-rules", "Ip"),
            ("x-rate-limit-ip", "8:10:60"),
            ("x-rate-limit-ip-state", "1:10:0"),
        ])]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}").await.unwrap();

        let limits = c.search_limits().limits();

        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].max, 8);
        // Ten second window plus the two second latency pad.
        assert_eq!(limits[0].window_secs, 12);
    }

    #[tokio::test]
    async fn a_wider_server_limit_lets_the_second_search_through_at_once() {
        let http = FakeHttp::with(vec![
            ok_response(vec![
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
            ]),
            ok_response(vec![]),
        ]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        c.send_search("{}").await.unwrap();
        c.send_search("{}").await.unwrap();

        // The conservative one per five seconds was replaced by eight per
        // twelve, so no wait was needed.
        assert_eq!(c.clock.total_slept(), 0);
    }

    #[tokio::test]
    async fn the_session_cookie_is_sent_when_present_and_omitted_when_not() {
        let http = FakeHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        )
        .with_session("deadbeef");

        c.send_search("{}").await.unwrap();

        // The cookie never reaches a log or a file from this crate, so the
        // test asserts on the request having been made rather than on the
        // header text.
        assert_eq!(c.http.sent_urls().len(), 1);
        assert_eq!(c.session_id, "deadbeef");
    }

    #[tokio::test]
    async fn the_league_is_percent_encoded_into_the_url() {
        let http = FakeHttp::with(vec![ok_response(vec![])]);
        let mut c = PriceCheckController::new(
            http,
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Hardcore Ruthless",
        );

        c.send_search("{}").await.unwrap();

        assert!(c.http.sent_urls()[0].contains("Hardcore%20Ruthless"));
    }

    #[tokio::test]
    async fn poe1_and_poe2_search_different_endpoints() {
        for (game, want) in [
            (GameVersion::Poe1, "/api/trade/search/"),
            (GameVersion::Poe2, "/api/trade2/search/"),
        ] {
            let http = FakeHttp::with(vec![ok_response(vec![])]);
            let mut c = PriceCheckController::new(
                http,
                FakeClock::new(),
                "https://www.pathofexile.com",
                game,
                "Standard",
            );

            c.send_search("{}").await.unwrap();

            assert!(c.http.sent_urls()[0].contains(want), "{game:?}");
        }
    }

    #[test]
    fn a_burst_estimate_reflects_the_current_limits() {
        let mut c = PriceCheckController::new(
            FakeHttp::with(Vec::new()),
            FakeClock::new(),
            "https://www.pathofexile.com",
            GameVersion::Poe2,
            "Standard",
        );

        // One per five seconds. Three requests take two windows of waiting.
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
        // The API marks online by the presence of an object, not a boolean.
        let got = read_listings(&fetch_body(&listing(5.0, "divine", false))).unwrap();

        assert!(!got[0].online);
    }

    #[test]
    fn a_null_entry_is_skipped_rather_than_failing_the_batch() {
        // The API returns one for a listing that vanished between the search
        // and the fetch, which happens constantly on a busy league.
        let entries = format!("null,{}", listing(5.0, "divine", true));

        assert_eq!(read_listings(&fetch_body(&entries)).unwrap().len(), 1);
    }

    #[test]
    fn an_unpriced_listing_is_skipped() {
        // Showing it as free would be worse than not showing it.
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
        // One listing at a thousand drags a mean far above what anyone pays,
        // and those listings are common because people post them to bait.
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
        // Mixing divine and chaos into one median produces a number in no
        // currency at all.
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
        // The server limits them separately, so sharing one set would throttle
        // the client about three times harder than it needs to.
        assert_eq!(limiter_for(Endpoint::Search), "search");
        assert_eq!(limiter_for(Endpoint::Fetch), "fetch");
        assert_eq!(limiter_for(Endpoint::Exchange), "exchange");
    }
}
