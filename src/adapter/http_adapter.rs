use std::collections::BTreeSet;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("network is disabled")]
    NetworkDisabled,

    #[error("host {host:?} is not allowed")]
    HostNotAllowed { host: String },

    #[error("url {url:?} has no host")]
    NoHost { url: String },

    #[error("url {url:?} uses scheme {scheme:?}")]
    BadScheme { url: String, scheme: String },
}

#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    enabled: bool,
    block_unlisted: bool,
    allowed_hosts: BTreeSet<String>,
}

impl NetworkPolicy {
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

    pub fn check(&self, url: &str) -> Result<(), PolicyError> {
        if !self.enabled {
            return Err(PolicyError::NetworkDisabled);
        }

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

    pub fn allowed_hosts(&self) -> Vec<&str> {
        self.allowed_hosts.iter().map(String::as_str).collect()
    }
}

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

fn host_of(url: &str) -> Result<String, PolicyError> {
    let no_host = || PolicyError::NoHost {
        url: url.to_string(),
    };

    let (_, rest) = url.split_once("://").ok_or_else(no_host)?;

    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .expect("split always yields one element");

    let hostport = match authority.rsplit_once('@') {
        Some((_, after)) => after,
        None => authority,
    };

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

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[allow(async_fn_in_trait)]
pub trait HttpClient: Send + Sync {
    async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, HttpAdapterError>;

    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpResponse, HttpAdapterError>;
}

#[derive(Debug, Error)]
pub enum HttpAdapterError {
    #[error("checking policy for {url:?}")]
    Policy {
        url: String,
        #[source]
        source: PolicyError,
    },

    #[error("requesting {url:?}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("reading response body from {url:?}")]
    Body {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

pub const DEFAULT_USER_AGENT: &str = concat!("poe-trader/", env!("CARGO_PKG_VERSION"));

pub struct HttpAdapter {
    policy: NetworkPolicy,
    client: reqwest::Client,
}

impl HttpAdapter {
    pub fn new(policy: NetworkPolicy, timeout: Duration) -> Result<Self, reqwest::Error> {
        Self::with_user_agent(policy, timeout, DEFAULT_USER_AGENT)
    }

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

        let redirect_policy = policy.clone();

        let client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(timeout)
            .user_agent(agent)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if redirect_policy.check(attempt.url().as_str()).is_err() {
                    return attempt.stop();
                }

                if attempt.previous().len() >= 10 {
                    return attempt.stop();
                }

                attempt.follow()
            }))
            .build()?;

        Ok(Self { policy, client })
    }

    pub fn policy(&self) -> &NetworkPolicy {
        &self.policy
    }

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

        assert!(p
            .check("https://evil.www.pathofexile.com.attacker.test/")
            .is_err());
    }

    #[test]
    fn userinfo_cannot_disguise_the_real_host() {
        let p = default_policy();

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
        assert!(DEFAULT_USER_AGENT.starts_with("poe-trader/"));
        assert!(DEFAULT_USER_AGENT.len() > "poe-trader/".len());
    }

    #[test]
    fn a_blank_configured_user_agent_falls_back_to_the_default() {
        let adapter =
            HttpAdapter::with_user_agent(default_policy(), Duration::from_secs(5), "   ").unwrap();

        assert_eq!(
            adapter.policy().allowed_hosts(),
            vec!["www.pathofexile.com"]
        );
    }

    #[test]
    fn the_policy_is_the_same_one_a_redirect_would_be_checked_against() {
        let policy = default_policy();
        let adapter = HttpAdapter::new(policy.clone(), Duration::from_secs(5)).unwrap();

        assert_eq!(adapter.policy().allowed_hosts(), policy.allowed_hosts());
        assert_eq!(
            adapter.policy().check("https://attacker.test/x"),
            policy.check("https://attacker.test/x")
        );
    }

    #[test]
    fn a_redirect_target_is_judged_by_the_same_rules_as_a_first_request() {
        let p = default_policy();

        assert!(p
            .check("https://www.pathofexile.com/api/trade2/search")
            .is_ok());
        assert!(p.check("https://attacker.test/steal").is_err());
        assert!(p.check("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(p.check("file:///etc/passwd").is_err());
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
