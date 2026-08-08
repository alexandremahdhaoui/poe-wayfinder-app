pub type Millis = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Limit {
    pub max: u32,
    pub window_secs: u32,
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    limit: Limit,
    released_at: Vec<Millis>,
}

impl RateLimiter {
    pub fn new(limit: Limit) -> Self {
        Self {
            limit,
            released_at: Vec::new(),
        }
    }

    pub fn limit(&self) -> Limit {
        self.limit
    }

    fn prune(&mut self, now: Millis) {
        self.released_at.retain(|&t| t > now);
    }

    pub fn available(&mut self, now: Millis) -> u32 {
        self.prune(now);

        self.limit.max.saturating_sub(self.released_at.len() as u32)
    }

    pub fn is_fully_utilized(&mut self, now: Millis) -> bool {
        self.available(now) == 0
    }

    pub fn in_use(&mut self, now: Millis) -> u32 {
        self.prune(now);

        self.released_at.len() as u32
    }

    pub fn borrow(&mut self, now: Millis) {
        self.prune(now);

        let released = now + u64::from(self.limit.window_secs) * 1000;

        let at = self.released_at.partition_point(|&t| t <= released);
        self.released_at.insert(at, released);
    }

    pub fn next_free(&mut self, now: Millis) -> Millis {
        if self.available(now) > 0 {
            return now;
        }

        self.released_at.first().copied().unwrap_or(now)
    }
}

#[derive(Debug, Clone, Default)]
pub struct LimiterSet {
    limiters: Vec<RateLimiter>,
}

impl LimiterSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn conservative() -> Self {
        Self {
            limiters: vec![RateLimiter::new(Limit {
                max: 1,
                window_secs: 5,
            })],
        }
    }

    pub fn limits(&self) -> Vec<Limit> {
        let mut out: Vec<Limit> = self.limiters.iter().map(RateLimiter::limit).collect();
        out.sort_unstable();

        out
    }

    pub fn wait_for(&mut self, now: Millis) -> Millis {
        self.limiters
            .iter_mut()
            .map(|l| l.next_free(now).saturating_sub(now))
            .max()
            .unwrap_or(0)
    }

    pub fn borrow(&mut self, now: Millis) {
        for l in &mut self.limiters {
            l.borrow(now);
        }
    }

    pub fn estimate_time(&mut self, count: u32, now: Millis, ignore_state: bool) -> Millis {
        let mut sim: Vec<(Limit, Vec<Millis>)> = self
            .limiters
            .iter_mut()
            .map(|l| {
                let stack = if ignore_state {
                    Vec::new()
                } else {
                    l.prune(now);
                    l.released_at.iter().map(|&t| t - now).collect()
                };

                (l.limit, stack)
            })
            .collect();

        let mut total: Millis = 0;

        for _ in 0..count {
            while sim
                .iter()
                .any(|(limit, stack)| stack.len() as u32 >= limit.max)
            {
                let wait = sim
                    .iter()
                    .filter(|(limit, stack)| stack.len() as u32 >= limit.max)
                    .filter_map(|(_, stack)| stack.first())
                    .map(|&first| first.saturating_sub(total))
                    .max()
                    .unwrap_or(0);

                total += wait;

                for (_, stack) in &mut sim {
                    stack.retain(|&t| t > total);
                }

                if wait == 0 {
                    break;
                }
            }

            for (limit, stack) in &mut sim {
                stack.push(total + u64::from(limit.window_secs) * 1000);
                stack.sort_unstable();
            }
        }

        total
    }

    pub fn adjust(&mut self, headers: &[(String, String)], latency_secs: u32, now: Millis) {
        let Some(server) = parse_rate_limit_headers(headers, latency_secs) else {
            return;
        };

        self.limiters
            .retain(|l| server.iter().any(|s| s.limit == l.limit));

        for l in &mut self.limiters {
            let Some(s) = server.iter().find(|s| s.limit == l.limit) else {
                continue;
            };

            let ours = l.in_use(now);

            for _ in ours..s.used {
                l.borrow(now);
            }
        }

        for s in &server {
            if self.limiters.iter().any(|l| l.limit == s.limit) {
                continue;
            }

            let mut l = RateLimiter::new(s.limit);

            for _ in 0..s.used {
                l.borrow(now);
            }

            self.limiters.push(l);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ServerLimit {
    limit: Limit,
    used: u32,
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn parse_rate_limit_headers(
    headers: &[(String, String)],
    latency_secs: u32,
) -> Option<Vec<ServerLimit>> {
    let rules = header(headers, "x-rate-limit-rules")?;

    let mut out = Vec::new();

    for rule in rules.split(',') {
        let rule = rule.trim();

        if rule.is_empty() {
            continue;
        }

        let limit_header = header(headers, &format!("x-rate-limit-{}", rule.to_lowercase()));
        let state_header = header(
            headers,
            &format!("x-rate-limit-{}-state", rule.to_lowercase()),
        );

        let (Some(limit_header), Some(state_header)) = (limit_header, state_header) else {
            continue;
        };

        let states: Vec<u32> = state_header
            .split(',')
            .map(|t| first_field(t).unwrap_or(0))
            .collect();

        for (i, triplet) in limit_header.split(',').enumerate() {
            let mut parts = triplet.trim().split(':');

            let (Some(max), Some(window)) = (
                parts.next().and_then(|p| p.trim().parse::<u32>().ok()),
                parts.next().and_then(|p| p.trim().parse::<u32>().ok()),
            ) else {
                continue;
            };

            out.push(ServerLimit {
                limit: Limit {
                    max,
                    window_secs: window + latency_secs,
                },
                used: states.get(i).copied().unwrap_or(0),
            });
        }
    }

    if out.is_empty() {
        return None;
    }

    Some(out)
}

fn first_field(triplet: &str) -> Option<u32> {
    triplet.trim().split(':').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Millis = 1000;

    fn hdr(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn limiter(max: u32, window_secs: u32) -> RateLimiter {
        RateLimiter::new(Limit { max, window_secs })
    }

    #[test]
    fn a_fresh_limiter_has_every_slot_free() {
        let mut l = limiter(8, 10);

        assert_eq!(l.available(0), 8);
        assert!(!l.is_fully_utilized(0));
    }

    #[test]
    fn borrowing_uses_a_slot() {
        let mut l = limiter(2, 10);

        l.borrow(0);

        assert_eq!(l.available(0), 1);
        assert_eq!(l.in_use(0), 1);
    }

    #[test]
    fn a_full_limiter_reports_itself_full() {
        let mut l = limiter(2, 10);

        l.borrow(0);
        l.borrow(0);

        assert!(l.is_fully_utilized(0));
        assert_eq!(l.available(0), 0);
    }

    #[test]
    fn a_slot_frees_only_after_the_whole_window() {
        let mut l = limiter(1, 10);

        l.borrow(0);

        assert!(l.is_fully_utilized(10 * SEC - 1));
        assert!(!l.is_fully_utilized(10 * SEC + 1));
    }

    #[test]
    fn a_slot_frees_exactly_at_the_end_of_its_window() {
        let mut l = limiter(1, 10);

        l.borrow(0);

        assert!(l.is_fully_utilized(10 * SEC - 1));
        assert!(!l.is_fully_utilized(10 * SEC));
    }

    #[test]
    fn slots_free_in_the_order_they_were_taken() {
        let mut l = limiter(2, 10);

        l.borrow(0);
        l.borrow(5 * SEC);

        assert_eq!(l.available(10 * SEC + 1), 1);
        assert_eq!(l.available(15 * SEC + 1), 2);
    }

    #[test]
    fn next_free_is_now_when_a_slot_is_free() {
        let mut l = limiter(2, 10);
        l.borrow(0);

        assert_eq!(l.next_free(0), 0);
    }

    #[test]
    fn next_free_is_the_earliest_release_when_full() {
        let mut l = limiter(2, 10);
        l.borrow(0);
        l.borrow(3 * SEC);

        assert_eq!(l.next_free(3 * SEC), 10 * SEC);
    }

    #[test]
    fn borrowing_past_the_max_never_reports_negative_headroom() {
        let mut l = limiter(1, 10);

        l.borrow(0);
        l.borrow(0);
        l.borrow(0);

        assert_eq!(l.available(0), 0);
        assert_eq!(l.in_use(0), 3);
    }

    #[test]
    fn an_empty_set_never_waits() {
        let mut set = LimiterSet::new();

        assert_eq!(set.wait_for(0), 0);
    }

    #[test]
    fn the_conservative_default_allows_one_request_every_five_seconds() {
        let mut set = LimiterSet::conservative();

        assert_eq!(set.wait_for(0), 0);

        set.borrow(0);

        assert_eq!(set.wait_for(0), 5 * SEC);
        assert_eq!(set.wait_for(5 * SEC), 0);
    }

    #[test]
    fn the_slowest_limiter_decides_the_wait() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(1, 5));
        set.limiters.push(limiter(1, 60));

        set.borrow(0);

        assert_eq!(set.wait_for(0), 60 * SEC);
    }

    #[test]
    fn one_rule_with_two_triplets_becomes_two_limiters() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60,15:60:120"),
                ("x-rate-limit-ip-state", "0:10:0,0:60:0"),
            ]),
            0,
            0,
        );

        assert_eq!(
            set.limits(),
            vec![
                Limit {
                    max: 8,
                    window_secs: 10
                },
                Limit {
                    max: 15,
                    window_secs: 60
                }
            ]
        );
    }

    #[test]
    fn two_rules_both_become_limiters() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip,Account"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
                ("x-rate-limit-account", "5:4:10"),
                ("x-rate-limit-account-state", "0:4:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limits().len(), 2);
    }

    #[test]
    fn the_configured_latency_widens_every_window() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
            ]),
            2,
            0,
        );

        assert_eq!(
            set.limits(),
            vec![Limit {
                max: 8,
                window_secs: 12
            }]
        );
    }

    #[test]
    fn the_server_state_is_loaded_into_a_new_limiter() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "3:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limiters[0].in_use(0), 3);
        assert_eq!(set.limiters[0].available(0), 5);
    }

    #[test]
    fn a_server_ahead_of_us_makes_us_catch_up() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));
        set.limiters[0].borrow(0);

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "5:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limiters[0].in_use(0), 5);
    }

    #[test]
    fn a_client_ahead_of_the_server_is_left_alone() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));

        for _ in 0..6 {
            set.limiters[0].borrow(0);
        }

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "2:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limiters[0].in_use(0), 6);
    }

    #[test]
    fn a_rule_the_server_dropped_is_destroyed() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(1, 5));
        set.limiters.push(limiter(8, 10));

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(
            set.limits(),
            vec![Limit {
                max: 8,
                window_secs: 10
            }]
        );
    }

    #[test]
    fn an_existing_limiter_is_reused_and_not_recreated() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));
        set.limiters[0].borrow(0);

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "1:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limiters.len(), 1);
        assert_eq!(set.limiters[0].in_use(0), 1);
    }

    #[test]
    fn a_response_with_no_rules_header_changes_nothing() {
        let mut set = LimiterSet::conservative();
        let before = set.limits();

        set.adjust(&hdr(&[("content-type", "application/json")]), 0, 0);

        assert_eq!(set.limits(), before);
    }

    #[test]
    fn header_names_are_matched_case_insensitively() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("X-Rate-Limit-Rules", "Ip"),
                ("X-Rate-Limit-Ip", "8:10:60"),
                ("X-Rate-Limit-Ip-State", "0:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limits().len(), 1);
    }

    #[test]
    fn a_rule_missing_its_limit_header_is_ignored() {
        let mut set = LimiterSet::conservative();
        let before = set.limits();

        set.adjust(&hdr(&[("x-rate-limit-rules", "Ip")]), 0, 0);

        assert_eq!(set.limits(), before);
    }

    #[test]
    fn a_malformed_triplet_is_ignored_and_the_rest_survive() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "garbage,8:10:60"),
                ("x-rate-limit-ip-state", "0:0:0,0:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(
            set.limits(),
            vec![Limit {
                max: 8,
                window_secs: 10
            }]
        );
    }

    #[test]
    fn an_entirely_malformed_header_leaves_the_current_rules_standing() {
        let mut set = LimiterSet::conservative();
        let before = set.limits();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "nonsense"),
                ("x-rate-limit-ip-state", "nonsense"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limits(), before);
    }

    #[test]
    fn the_same_rule_under_two_names_is_counted_once() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip,Account"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
                ("x-rate-limit-account", "8:10:60"),
                ("x-rate-limit-account-state", "0:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limits().len(), 1);
    }

    #[test]
    fn an_empty_rule_name_is_skipped() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip,"),
                ("x-rate-limit-ip", "8:10:60"),
                ("x-rate-limit-ip-state", "0:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limits().len(), 1);
    }

    #[test]
    fn a_state_header_shorter_than_the_limit_header_reads_as_unused() {
        let mut set = LimiterSet::new();

        set.adjust(
            &hdr(&[
                ("x-rate-limit-rules", "Ip"),
                ("x-rate-limit-ip", "8:10:60,15:60:120"),
                ("x-rate-limit-ip-state", "3:10:0"),
            ]),
            0,
            0,
        );

        assert_eq!(set.limiters[0].in_use(0), 3);
        assert_eq!(set.limiters[1].in_use(0), 0);
    }

    #[test]
    fn a_burst_inside_the_allowance_takes_no_time() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));

        assert_eq!(set.estimate_time(8, 0, false), 0);
    }

    #[test]
    fn a_burst_past_the_allowance_waits_a_window() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));

        assert_eq!(set.estimate_time(9, 0, false), 10 * SEC);
    }

    #[test]
    fn estimating_accounts_for_slots_already_held() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));

        for _ in 0..8 {
            set.limiters[0].borrow(0);
        }

        assert_eq!(set.estimate_time(1, 0, false), 10 * SEC);
    }

    #[test]
    fn ignoring_state_estimates_a_clean_run() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));

        for _ in 0..8 {
            set.limiters[0].borrow(0);
        }

        assert_eq!(set.estimate_time(1, 0, true), 0);
    }

    #[test]
    fn estimating_zero_requests_takes_no_time() {
        let mut set = LimiterSet::conservative();

        assert_eq!(set.estimate_time(0, 0, false), 0);
    }

    #[test]
    fn estimating_on_an_empty_set_takes_no_time() {
        let mut set = LimiterSet::new();

        assert_eq!(set.estimate_time(100, 0, false), 0);
    }

    #[test]
    fn a_zero_second_window_does_not_hang_the_estimate() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(1, 0));

        assert_eq!(set.estimate_time(5, 0, false), 0);
    }

    #[test]
    fn the_tighter_of_two_rules_drives_the_estimate() {
        let mut set = LimiterSet::new();
        set.limiters.push(limiter(8, 10));
        set.limiters.push(limiter(2, 4));

        assert_eq!(set.estimate_time(4, 0, false), 4 * SEC);
    }
}
