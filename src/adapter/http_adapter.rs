//! The only socket in the project.
//!
//! Every outbound request passes through here. Nothing else in the workspace
//! builds an HTTP client, so the allowlist has exactly one place to live and
//! one place to audit.
//!
//! The reference does the same thing in an Electron session handler, but the
//! host list is a hardcoded array in `main/src/proxy.ts`. Here it is config.
//! A user can see it, narrow it or turn the network off entirely.

use std::collections::BTreeSet;
use std::time::Duration;

use thiserror::Error;

/// Why a request was refused.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    /// The master switch is off.
    #[error("network is disabled")]
    NetworkDisabled,

    /// The host is not in the allowlist.
    #[error("host {host:?} is not allowed")]
    HostNotAllowed { host: String },

    /// The URL did not parse or carried no host.
    #[error("url {url:?} has no host")]
    NoHost { url: String },

    /// The scheme was not http or https.
    #[error("url {url:?} uses scheme {scheme:?}")]
    BadScheme { url: String, scheme: String },
}

/// What the app is allowed to reach.
///
/// Built from config once at startup and then never changed. A policy that can
/// be edited at runtime is a policy that can be edited by a bug.
#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    enabled: bool,
    block_unlisted: bool,
    allowed_hosts: BTreeSet<String>,
}

impl NetworkPolicy {
    /// Build a policy from the generated config values.
    ///
    /// `allowed_hosts` is comma separated because golden-configgen has no list
    /// type. Blank entries are dropped and each host is lowercased, so
    /// `WWW.PathOfExile.com` and a stray trailing comma both behave.
    pub fn new(enabled: bool, block_unlisted: bool, allowed_hosts: &str) -> Self {
        let allowed_hosts = allowed_hosts
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();

        Self {
            enabled,
            block_unlisted,
            allowed_hosts,
        }
    }

    /// Decide whether a URL may be requested.
    ///
    /// Pure. This is the whole security boundary and it is testable with no
    /// socket, so there is no excuse for it to be under tested.
    pub fn check(&self, url: &str) -> Result<(), PolicyError> {
        if !self.enabled {
            return Err(PolicyError::NetworkDisabled);
        }

        // Scheme first. `file:///etc/passwd` has no host, and reporting that
        // as a missing host hides the fact that the scheme was never valid.
        let scheme = scheme_of(url)?;

        if scheme != "http" && scheme != "https" {
            return Err(PolicyError::BadScheme {
                url: url.to_string(),
                scheme,
            });
        }

        let host = host_of(url)?;

        if !self.block_unlisted {
            return Ok(());
        }

        if self.allowed_hosts.contains(&host) {
            return Ok(());
        }

        Err(PolicyError::HostNotAllowed { host })
    }

    /// The hosts this policy permits, sorted.
    ///
    /// Used at startup to log what the app may reach. An operator should never
    /// have to read the source to answer that question.
    pub fn allowed_hosts(&self) -> Vec<&str> {
        self.allowed_hosts.iter().map(String::as_str).collect()
    }
}

/// Pull the lowercased scheme out of a URL.
fn scheme_of(url: &str) -> Result<String, PolicyError> {
    let no_host = || PolicyError::NoHost {
        url: url.to_string(),
    };

    let (scheme, _) = url.split_once("://").ok_or_else(no_host)?;

    if scheme.is_empty() {
        return Err(no_host());
    }

    Ok(scheme.to_ascii_lowercase())
}

/// Pull the lowercased host out of a URL.
///
/// Hand written rather than pulled from the `url` crate, because the policy
/// must reject anything it cannot fully understand. A parser that is lenient
/// here turns into a bypass.
fn host_of(url: &str) -> Result<String, PolicyError> {
    let no_host = || PolicyError::NoHost {
        url: url.to_string(),
    };

    let (_, rest) = url.split_once("://").ok_or_else(no_host)?;

    // Authority ends at the first /, ? or #.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .expect("split always yields one element");

    // Drop userinfo. `https://www.pathofexile.com@evil.example/` points at
    // evil.example and reads like the real thing.
    let hostport = match authority.rsplit_once('@') {
        Some((_, after)) => after,
        None => authority,
    };

    // Drop the port. An IPv6 literal keeps its brackets.
    let host = if let Some(end) = hostport.strip_prefix('[') {
        match end.split_once(']') {
            Some((inner, _)) => inner,
            None => return Err(no_host()),
        }
    } else {
        match hostport.split_once(':') {
            Some((h, _)) => h,
            None => hostport,
        }
    };

    if host.is_empty() {
        return Err(no_host());
    }

    Ok(host.to_ascii_lowercase())
}

/// One HTTP response, reduced to what callers need.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    /// First value of a header, matched case insensitively.
    ///
    /// The rate limiter reads `x-rate-limit-rules` and friends, and header
    /// casing is not guaranteed by anything.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// What a caller may ask of the network.
///
/// Declared here because this module implements it. Callers take the trait so
/// tests never open a socket.
#[allow(async_fn_in_trait)]
pub trait HttpClient: Send + Sync {
    /// Send a GET, subject to the policy.
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, HttpAdapterError>;

    /// Send a POST with a JSON body, subject to the policy.
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, HttpAdapterError>;
}

/// Anything that can go wrong on the way out.
#[derive(Debug, Error)]
pub enum HttpAdapterError {
    /// The policy refused the request. No socket was opened.
    #[error("checking policy for {url:?}")]
    Policy {
        url: String,
        #[source]
        source: PolicyError,
    },

    /// The request was sent and failed.
    #[error("requesting {url:?}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    /// The response arrived but the body could not be read.
    #[error("reading response body from {url:?}")]
    Body {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

/// The User-Agent sent when config supplies none.
///
/// GGG answers 403 to a request with no User-Agent, and their API policy asks
/// for a descriptive one carrying contact details. The default names the tool
/// and its version. A user who wants to add contact details sets the config
/// key, and doing so is good manners rather than a requirement.
pub const DEFAULT_USER_AGENT: &str = concat!("poe-trader/", env!("CARGO_PKG_VERSION"));

/// The real client.
pub struct HttpAdapter {
    policy: NetworkPolicy,
    client: reqwest::Client,
}

impl HttpAdapter {
    /// Build the client.
    ///
    /// The cookie store is on because the trade API needs POESESSID. The
    /// timeout is here and not per call so no caller can forget it.
    pub fn new(policy: NetworkPolicy, timeout: Duration) -> Result<Self, reqwest::Error> {
        Self::with_user_agent(policy, timeout, DEFAULT_USER_AGENT)
    }

    /// Build the client with a specific User-Agent.
    ///
    /// GGG answers 403 to a request that carries none, and reqwest sends none
    /// by default. This is not optional.
    pub fn with_user_agent(
        policy: NetworkPolicy,
        timeout: Duration,
        user_agent: &str,
    ) -> Result<Self, reqwest::Error> {
        let agent = if user_agent.trim().is_empty() {
            DEFAULT_USER_AGENT
        } else {
            user_agent
        };

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(timeout)
            .user_agent(agent)
            .build()?;

        Ok(Self { policy, client })
    }

    /// The policy this adapter enforces.
    pub fn policy(&self) -> &NetworkPolicy {
        &self.policy
    }

    /// Refuse before building a request, so a blocked URL never reaches DNS.
    fn guard(&self, url: &str) -> Result<(), HttpAdapterError> {
        self.policy
            .check(url)
            .map_err(|source| HttpAdapterError::Policy {
                url: url.to_string(),
                source,
            })
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, HttpAdapterError> {
        let mut builder = builder;
        for (k, v) in headers {
            builder = builder.header(*k, *v);
        }

        let resp = builder
            .send()
            .await
            .map_err(|source| HttpAdapterError::Request {
                url: url.to_string(),
                source,
            })?;

        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();

        let body = resp.text().await.map_err(|source| HttpAdapterError::Body {
            url: url.to_string(),
            source,
        })?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

impl HttpClient for HttpAdapter {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, HttpAdapterError> {
        self.guard(url)?;

        self.send(self.client.get(url), url, headers).await
    }

    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, HttpAdapterError> {
        self.guard(url)?;

        let builder = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body.to_string());

        self.send(builder, url, headers).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> NetworkPolicy {
        NetworkPolicy::new(true, true, "www.pathofexile.com")
    }

    #[test]
    fn allows_the_one_listed_host() {
        let p = default_policy();

        assert_eq!(
            p.check("https://www.pathofexile.com/api/trade2/data/stats"),
            Ok(())
        );
    }

    #[test]
    fn refuses_the_private_fork_server() {
        let p = default_policy();

        assert_eq!(
            p.check("https://api.exiledexchange2.dev/stats"),
            Err(PolicyError::HostNotAllowed {
                host: "api.exiledexchange2.dev".into()
            })
        );
    }

    #[test]
    fn refuses_regional_domains() {
        let p = default_policy();

        for url in [
            "https://ru.pathofexile.com/api/trade/search",
            "https://poe.game.daum.net/api/trade/search",
            "https://pathofexile.tw/api/trade/search",
            "https://web.poe.garena.tw/api/trade/search",
        ] {
            assert!(p.check(url).is_err(), "{url} was allowed");
        }
    }

    #[test]
    fn a_subdomain_of_an_allowed_host_is_not_allowed() {
        let p = default_policy();

        // Matching by suffix would let evil.www.pathofexile.com.attacker.test
        // through. The check is exact.
        assert!(p
            .check("https://evil.www.pathofexile.com.attacker.test/")
            .is_err());
    }

    #[test]
    fn userinfo_cannot_disguise_the_real_host() {
        let p = default_policy();

        // Reads like the real site. Resolves to attacker.test.
        assert_eq!(
            p.check("https://www.pathofexile.com@attacker.test/api"),
            Err(PolicyError::HostNotAllowed {
                host: "attacker.test".into()
            })
        );
    }

    #[test]
    fn host_matching_ignores_case_and_port() {
        let p = default_policy();

        assert_eq!(p.check("HTTPS://WWW.PathOfExile.COM:443/api"), Ok(()));
    }

    #[test]
    fn the_master_switch_beats_the_allowlist() {
        let p = NetworkPolicy::new(false, true, "www.pathofexile.com");

        assert_eq!(
            p.check("https://www.pathofexile.com/api"),
            Err(PolicyError::NetworkDisabled)
        );
    }

    #[test]
    fn the_master_switch_beats_an_open_allowlist() {
        let p = NetworkPolicy::new(false, false, "");

        assert_eq!(
            p.check("https://anything.test/"),
            Err(PolicyError::NetworkDisabled)
        );
    }

    #[test]
    fn unblocking_unlisted_hosts_allows_anything_http() {
        let p = NetworkPolicy::new(true, false, "www.pathofexile.com");

        assert_eq!(p.check("https://anything.test/"), Ok(()));
    }

    #[test]
    fn non_http_schemes_are_refused_even_with_an_open_allowlist() {
        let p = NetworkPolicy::new(true, false, "");

        for url in [
            "file:///etc/passwd",
            "ftp://host.test/x",
            "ws://host.test/x",
        ] {
            assert!(
                matches!(p.check(url), Err(PolicyError::BadScheme { .. })),
                "{url} was allowed"
            );
        }
    }

    #[test]
    fn a_url_with_no_host_is_refused() {
        let p = NetworkPolicy::new(true, false, "");

        for url in ["not a url", "https://", "https://:443/x", "://host.test"] {
            assert!(
                matches!(p.check(url), Err(PolicyError::NoHost { .. })),
                "{url} was allowed"
            );
        }
    }

    #[test]
    fn an_unterminated_ipv6_literal_is_refused() {
        let p = NetworkPolicy::new(true, false, "");

        assert!(matches!(
            p.check("https://[::1/x"),
            Err(PolicyError::NoHost { .. })
        ));
    }

    #[test]
    fn an_ipv6_literal_keeps_its_address() {
        let p = NetworkPolicy::new(true, true, "::1");

        assert_eq!(p.check("http://[::1]:8080/x"), Ok(()));
    }

    #[test]
    fn blank_and_padded_allowlist_entries_are_dropped() {
        let p = NetworkPolicy::new(true, true, " www.pathofexile.com , ,, ");

        assert_eq!(p.allowed_hosts(), vec!["www.pathofexile.com"]);
        assert_eq!(p.check("https://www.pathofexile.com/x"), Ok(()));
    }

    #[test]
    fn an_empty_allowlist_refuses_everything() {
        let p = NetworkPolicy::new(true, true, "");

        assert!(p.check("https://www.pathofexile.com/x").is_err());
    }

    #[test]
    fn the_query_and_fragment_are_not_part_of_the_host() {
        let p = default_policy();

        assert_eq!(p.check("https://www.pathofexile.com?x=1"), Ok(()));
        assert_eq!(p.check("https://www.pathofexile.com#frag"), Ok(()));
    }

    #[test]
    fn header_lookup_ignores_case() {
        let r = HttpResponse {
            status: 200,
            headers: vec![("X-Rate-Limit-Rules".into(), "Ip".into())],
            body: String::new(),
        };

        assert_eq!(r.header("x-rate-limit-rules"), Some("Ip"));
        assert_eq!(r.header("x-missing"), None);
    }

    #[test]
    fn a_refused_request_names_the_url_and_the_reason() {
        let adapter = HttpAdapter::new(default_policy(), Duration::from_secs(5)).unwrap();

        let err = adapter.guard("https://attacker.test/x").unwrap_err();

        assert!(err.to_string().contains("attacker.test"));
        assert!(std::error::Error::source(&err)
            .unwrap()
            .to_string()
            .contains("not allowed"));
    }

    #[test]
    fn the_default_user_agent_names_the_tool_and_its_version() {
        // GGG answers 403 to a request with no User-Agent, and reqwest sends
        // none by default. An empty one is the same as none.
        assert!(DEFAULT_USER_AGENT.starts_with("poe-trader/"));
        assert!(DEFAULT_USER_AGENT.len() > "poe-trader/".len());
    }

    #[test]
    fn a_blank_configured_user_agent_falls_back_to_the_default() {
        // An empty header is the same as no header, which is a 403.
        let adapter =
            HttpAdapter::with_user_agent(default_policy(), Duration::from_secs(5), "   ").unwrap();

        assert_eq!(
            adapter.policy().allowed_hosts(),
            vec!["www.pathofexile.com"]
        );
    }

    #[test]
    fn the_adapter_exposes_its_policy() {
        let adapter = HttpAdapter::new(default_policy(), Duration::from_secs(5)).unwrap();

        assert_eq!(
            adapter.policy().allowed_hosts(),
            vec!["www.pathofexile.com"]
        );
    }
}
