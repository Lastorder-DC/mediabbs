//! Flood control for posting (topics, comments, uploads): at most
//! `POSTS_PER_MINUTE` per client address in any 60 seconds. In memory, per
//! backend process.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(60);

pub struct PostLimiter {
    per_minute: usize,
    hits: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl PostLimiter {
    /// `per_minute` 0 turns the limit off.
    pub fn new(per_minute: usize) -> Self {
        PostLimiter {
            per_minute,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Counts a post from `ip`; `Err(seconds to wait)` when over the limit.
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        if self.per_minute == 0 {
            return Ok(());
        }
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        // Forget idle addresses now and then so the map stays small.
        if hits.len() > 10_000 {
            hits.retain(|_, times| times.back().is_some_and(|t| now - *t < WINDOW));
        }
        let times = hits.entry(ip).or_default();
        while times.front().is_some_and(|t| now - *t >= WINDOW) {
            times.pop_front();
        }
        if times.len() >= self.per_minute {
            let oldest = *times.front().expect("not empty");
            return Err((WINDOW - (now - oldest)).as_secs().max(1));
        }
        times.push_back(now);
        Ok(())
    }
}

/// The response for a refused post.
pub fn too_many(retry_after: u64) -> actix_web::HttpResponse {
    actix_web::HttpResponse::TooManyRequests()
        .insert_header(("Retry-After", retry_after.to_string()))
        .body("Too many posts")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit() {
        let limiter = PostLimiter::new(2);
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        let t0 = Instant::now();
        assert!(limiter.check_at(a, t0).is_ok());
        assert!(limiter.check_at(a, t0 + Duration::from_secs(1)).is_ok());
        let wait = limiter
            .check_at(a, t0 + Duration::from_secs(2))
            .unwrap_err();
        assert!((57..=58).contains(&wait), "{}", wait);
        assert!(limiter.check_at(b, t0).is_ok(), "per address");
        assert!(
            limiter.check_at(a, t0 + Duration::from_secs(61)).is_ok(),
            "window slides"
        );
    }

    #[test]
    fn test_off() {
        let limiter = PostLimiter::new(0);
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..100 {
            assert!(limiter.check(a).is_ok());
        }
    }
}
