// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use std::time::{Duration, SystemTime};
/// Application-observed admission state; portable even where OS hints are advisory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetworkState {
    #[default]
    Available,
    Offline,
    Constrained,
    Expensive,
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub max_response_bytes: usize,
    pub max_upload_bytes: usize,
    pub idle_timeout: Duration,
    pub request_timeout: Duration,
    pub allow_constrained: bool,
    pub allow_expensive: bool,
    pub retry: Retry,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_response_bytes: 8 * 1024 * 1024,
            max_upload_bytes: 8 * 1024 * 1024,
            idle_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(90),
            allow_constrained: false,
            allow_expensive: false,
            retry: Retry::default(),
        }
    }
}
/// Read retries only. Long Retry-After delays are returned to the durable scheduler.
#[derive(Clone, Debug)]
pub struct Retry {
    pub max_attempts: u8,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_elapsed: Duration,
    pub jitter_seed: u64,
}
impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(15),
            max_elapsed: Duration::from_secs(120),
            jitter_seed: day_part_http::simulation::Clock::now(
                &day_part_http::simulation::RealClock,
            )
            .as_nanos() as u64,
        }
    }
}
impl Retry {
    pub(crate) fn delay(
        &self,
        attempt: u8,
        retry_after: Option<Duration>,
        salt: u64,
    ) -> Option<Duration> {
        if attempt >= self.max_attempts.max(1) {
            return None;
        }
        let cap = self
            .base_delay
            .saturating_mul(1u32 << u32::from(attempt.min(16)))
            .min(self.max_delay);
        let mut x = self.jitter_seed ^ salt ^ u64::from(attempt).wrapping_mul(0x9e3779b97f4a7c15);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let delay = cap / 2 + cap.mul_f64((x % 1001) as f64 / 2000.0);
        let delay = delay.max(retry_after.unwrap_or_default());
        (delay <= self.max_delay).then_some(delay)
    }
}
pub(crate) fn retry_after(value: Option<&str>, now: Option<SystemTime>) -> Option<Duration> {
    let value = value?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(value).ok().map(|t| {
        now.map(|now| t.duration_since(now).unwrap_or_default())
            .unwrap_or(Duration::MAX)
    })
}

pub(crate) fn wall_now(response: &day_part_http::Response) -> Option<SystemTime> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = response;
        Some(SystemTime::now())
    }
    #[cfg(target_arch = "wasm32")]
    {
        response
            .header("date")
            .and_then(|v| httpdate::parse_http_date(v).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_dates_require_a_clock_and_never_shorten_a_server_delay() {
        let now = httpdate::parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:28:10 GMT"), Some(now)),
            Some(Duration::from_secs(10))
        );
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2015 07:28:10 GMT"), None),
            Some(Duration::MAX)
        );
        assert_eq!(retry_after(Some("12"), None), Some(Duration::from_secs(12)));
        assert_eq!(retry_after(Some("invalid"), Some(now)), None);
    }
}
