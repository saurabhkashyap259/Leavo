//! PostgreSQL-backed sliding-window rate limits (FR-003, research R4) and client-IP extraction.
//!
//! Each allowed request is recorded as a row in `rate_limits`; a rule rejects once the count
//! inside its window reaches the maximum. Limits hold across replicas and restarts.
use axum::http::HeaderMap;
use chrono::{DateTime, Duration, Utc};
use ipnet::IpNet;
use sqlx::PgConnection;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub window: Duration,
    pub max: i64,
}

pub const CODE_PER_EMAIL: &[Rule] = &[
    Rule {
        window: Duration::seconds(30),
        max: 1,
    },
    Rule {
        window: Duration::hours(1),
        max: 5,
    },
];
pub const CODE_PER_IP: &[Rule] = &[Rule {
    window: Duration::hours(1),
    max: 20,
}];

/// `Err(seconds)` if any rule is exhausted, with how long until it frees up.
pub async fn check(
    conn: &mut PgConnection,
    bucket: &str,
    key: &str,
    now: DateTime<Utc>,
    rules: &[Rule],
) -> sqlx::Result<Result<(), u64>> {
    let mut wait = 0i64;
    for rule in rules {
        let (count, oldest): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT COALESCE(SUM(count), 0)::bigint, min(window_start) FROM rate_limits
             WHERE bucket = $1 AND key = $2 AND window_start > $3",
        )
        .bind(bucket)
        .bind(key)
        .bind(now - rule.window)
        .fetch_one(&mut *conn)
        .await?;
        if count >= rule.max {
            let free_at = oldest.unwrap_or(now) + rule.window;
            wait = wait.max((free_at - now).num_seconds().max(1));
        }
    }
    Ok(if wait > 0 { Err(wait as u64) } else { Ok(()) })
}

pub async fn record(
    conn: &mut PgConnection,
    bucket: &str,
    key: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO rate_limits (bucket, key, window_start, count) VALUES ($1, $2, $3, 1)
         ON CONFLICT (bucket, key, window_start) DO UPDATE SET count = rate_limits.count + 1",
    )
    .bind(bucket)
    .bind(key)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// The client's IP. `X-Forwarded-For` is trusted only when the direct peer is a configured
/// proxy; then the right-most address that is not itself a trusted proxy is used.
pub fn client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|n| n.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    forwarded
        .into_iter()
        .rev()
        .find(|ip| !is_trusted(ip))
        .unwrap_or(peer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(xff: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", HeaderValue::from_str(xff).unwrap());
        h
    }

    #[test]
    fn untrusted_peer_cannot_spoof_its_address() {
        let peer: IpAddr = "203.0.113.9".parse().unwrap();
        assert_eq!(client_ip(peer, &headers("1.2.3.4"), &[]), peer);
    }

    #[test]
    fn trusted_proxy_forwards_the_real_client() {
        let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
        let peer: IpAddr = "10.0.0.5".parse().unwrap();
        let ip = client_ip(peer, &headers("1.2.3.4, 198.51.100.7, 10.0.0.2"), &trusted);
        assert_eq!(
            ip,
            "198.51.100.7".parse::<IpAddr>().unwrap(),
            "right-most untrusted hop"
        );
    }

    #[test]
    fn trusted_proxy_without_header_falls_back_to_peer() {
        let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
        let peer: IpAddr = "10.0.0.5".parse().unwrap();
        assert_eq!(client_ip(peer, &HeaderMap::new(), &trusted), peer);
    }
}
