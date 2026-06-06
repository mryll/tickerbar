use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::platform::model::{FetchError, Quote, QuoteState};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Record {
    schema_version: u32,
    fetched_at: DateTime<Utc>,
    quotes: Vec<Quote>,
    backoff_until: Option<DateTime<Utc>>,
}

pub fn cache_dir() -> PathBuf {
    let d = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("tickerbar");
    fs::create_dir_all(&d).ok();
    d
}

/// Filesystem-safe cache file derived from the request key + schema version.
pub fn key_file(dir: &Path, key: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    SCHEMA_VERSION.hash(&mut h);
    key.hash(&mut h);
    dir.join(format!("{:016x}.json", h.finish()))
}

fn read_record(path: &Path) -> Option<Record> {
    let body = fs::read_to_string(path).ok()?;
    let rec: Record = serde_json::from_str(&body).ok()?;
    if rec.schema_version != SCHEMA_VERSION {
        return None;
    }
    Some(rec)
}

fn write_record(path: &Path, rec: &Record) {
    if let Ok(body) = serde_json::to_string(rec) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, body).is_ok() {
            fs::rename(&tmp, path).ok();
        }
    }
}

fn served_stale(mut quotes: Vec<Quote>) -> Vec<Quote> {
    for q in &mut quotes {
        q.state = QuoteState::Stale;
    }
    quotes
}

fn unlock(lock: Option<fs::File>) {
    if let Some(l) = lock {
        FileExt::unlock(&l).ok();
    }
}

/// Caching wrapper. `fetch_fn` is the only seam to the network — fake it in tests.
///
/// - Fresh within TTL: return cached quotes, no fetch.
/// - Lock held by another instance: serve stale (or empty), no fetch — never block.
/// - Inside a persisted 429 backoff window: serve stale, no fetch.
/// - Otherwise fetch: persist on success; on 429 persist a backoff window and serve stale;
///   on other error serve stale; with no cache return empty.
pub fn get_or_fetch<F>(
    dir: &Path,
    key: &str,
    ttl: Duration,
    now: DateTime<Utc>,
    fetch_fn: F,
) -> Vec<Quote>
where
    F: FnOnce() -> Result<Vec<Quote>, FetchError>,
{
    let path = key_file(dir, key);
    let lock_path = path.with_extension("lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .ok();
    let have_lock = lock
        .as_ref()
        .map(|l| l.try_lock_exclusive().is_ok())
        .unwrap_or(false);

    let existing = read_record(&path);

    if !have_lock {
        return existing.map(|r| served_stale(r.quotes)).unwrap_or_default();
    }

    if let Some(rec) = &existing {
        if now.signed_duration_since(rec.fetched_at) < ttl {
            unlock(lock);
            return rec.quotes.clone();
        }
        if let Some(until) = rec.backoff_until {
            if now < until {
                unlock(lock);
                return served_stale(rec.quotes.clone());
            }
        }
    }

    let out = match fetch_fn() {
        Ok(quotes) => {
            write_record(
                &path,
                &Record {
                    schema_version: SCHEMA_VERSION,
                    fetched_at: now,
                    quotes: quotes.clone(),
                    backoff_until: None,
                },
            );
            quotes
        }
        Err(FetchError::RateLimited { retry_after }) => {
            let secs = retry_after.unwrap_or(300) as i64;
            match existing {
                Some(rec) => {
                    // Keep the OLD fetched_at so stale age keeps growing.
                    write_record(
                        &path,
                        &Record {
                            schema_version: SCHEMA_VERSION,
                            fetched_at: rec.fetched_at,
                            quotes: rec.quotes.clone(),
                            backoff_until: Some(now + Duration::seconds(secs)),
                        },
                    );
                    served_stale(rec.quotes)
                }
                None => Vec::new(),
            }
        }
        Err(FetchError::Other(_)) => existing.map(|r| served_stale(r.quotes)).unwrap_or_default(),
    };
    unlock(lock);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::model::*;
    use chrono::{Duration, Utc};

    fn q(price: f64, now: chrono::DateTime<Utc>) -> Quote {
        Quote {
            label: "BTC".into(),
            base: "btc".into(),
            quote: "usd".into(),
            native_quote: "usd".into(),
            price: Some(price),
            change_pct: Some(1.0),
            change_abs: None,
            direction: Some(Direction::Up),
            source: ProviderKind::CoinGecko,
            as_of: None,
            fetched_at: now,
            state: QuoteState::Fresh,
        }
    }

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tickerbar-test-{}", nanos()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn a_successful_fetch_is_cached_and_returned_fresh() {
        let dir = tempdir();
        let now = Utc::now();
        let out = get_or_fetch(&dir, "k1", Duration::seconds(60), now, || {
            Ok(vec![q(100.0, now)])
        });
        assert_eq!(out[0].state, QuoteState::Fresh);
        assert_eq!(out[0].price, Some(100.0));
    }

    #[test]
    fn a_fresh_cache_within_ttl_skips_the_fetch() {
        let dir = tempdir();
        let now = Utc::now();
        let _ = get_or_fetch(&dir, "k7", Duration::seconds(60), now, || {
            Ok(vec![q(10.0, now)])
        });
        let mut called = false;
        let out = get_or_fetch(&dir, "k7", Duration::seconds(60), now, || {
            called = true;
            Ok(vec![q(99.0, now)])
        });
        assert!(!called, "fetch must be skipped when cache is fresh");
        assert_eq!(out[0].price, Some(10.0));
    }

    #[test]
    fn a_provider_error_falls_back_to_stale_cache_marked_stale() {
        let dir = tempdir();
        let t0 = Utc::now() - Duration::seconds(120);
        let _ = get_or_fetch(&dir, "k2", Duration::seconds(60), t0, || {
            Ok(vec![q(100.0, t0)])
        });
        let now = Utc::now();
        let out = get_or_fetch(&dir, "k2", Duration::seconds(60), now, || {
            Err(FetchError::Other("down".into()))
        });
        assert_eq!(out[0].state, QuoteState::Stale);
        assert_eq!(out[0].price, Some(100.0));
    }

    #[test]
    fn a_rate_limited_provider_serves_stale_and_persists_a_backoff_window() {
        let dir = tempdir();
        let t0 = Utc::now();
        let _ = get_or_fetch(&dir, "k3", Duration::seconds(0), t0, || {
            Ok(vec![q(100.0, t0)])
        });
        let out = get_or_fetch(&dir, "k3", Duration::seconds(0), Utc::now(), || {
            Err(FetchError::RateLimited {
                retry_after: Some(300),
            })
        });
        assert_eq!(out[0].state, QuoteState::Stale);
        let mut called = false;
        let out2 = get_or_fetch(&dir, "k3", Duration::seconds(0), Utc::now(), || {
            called = true;
            Ok(vec![])
        });
        assert!(!called, "fetch must be skipped during backoff");
        assert_eq!(out2[0].state, QuoteState::Stale);
    }

    #[test]
    fn a_held_lock_serves_stale_without_fetching() {
        let dir = tempdir();
        let t0 = Utc::now() - Duration::seconds(120);
        let _ = get_or_fetch(&dir, "k6", Duration::seconds(60), t0, || {
            Ok(vec![q(50.0, t0)])
        });
        let lockp = key_file(&dir, "k6").with_extension("lock");
        let f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lockp)
            .unwrap();
        FileExt::lock_exclusive(&f).unwrap();
        let mut called = false;
        let out = get_or_fetch(&dir, "k6", Duration::seconds(0), Utc::now(), || {
            called = true;
            Ok(vec![])
        });
        assert!(!called, "fetch must be skipped when the lock is held");
        assert_eq!(out[0].state, QuoteState::Stale);
        FileExt::unlock(&f).ok();
    }

    #[test]
    fn no_cache_and_a_failed_fetch_yields_an_empty_result() {
        let dir = tempdir();
        let out = get_or_fetch(&dir, "k4", Duration::seconds(60), Utc::now(), || {
            Err(FetchError::Other("x".into()))
        });
        assert!(out.is_empty());
    }

    #[test]
    fn a_corrupt_cache_file_is_ignored_and_the_fetch_proceeds() {
        let dir = tempdir();
        let path = key_file(&dir, "k5");
        std::fs::write(&path, b"{not json").unwrap();
        let now = Utc::now();
        let out = get_or_fetch(&dir, "k5", Duration::seconds(60), now, || {
            Ok(vec![q(7.0, now)])
        });
        assert_eq!(out[0].price, Some(7.0));
    }
}
