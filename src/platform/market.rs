use chrono::{DateTime, Datelike, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

use crate::platform::config::MarketHours;
use crate::platform::model::AssetSource;

pub enum Gate {
    Open,
    Closed {
        last_close: DateTime<Utc>,
        /// Start of the session that produced `last_close` (its `open`, in UTC).
        session_start: DateTime<Utc>,
        mode: ClosedCacheMode,
    },
}

/// How a provider's cache should behave while its market is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedCacheMode {
    /// API still serves the last close/value off-hours — a cache fetched any time after the close
    /// is the legitimate last close (most providers).
    LatestSnapshot,
    /// Live-only feed that empties outside the session (data912) — only a cache captured during the
    /// session window counts; off-session fetches return empty and must never be frozen.
    LiveSessionOnly,
}

/// Weekday session, with the feed delay + grace already baked into `fetch_close` so we keep
/// polling until the real close has arrived (e.g. data912 is ~2h delayed).
struct MarketSpec {
    tz: Tz,
    open: NaiveTime,
    fetch_close: NaiveTime,
    closed_cache_mode: ClosedCacheMode,
}

fn at(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
}

fn spec(source: &AssetSource) -> Option<MarketSpec> {
    let ba = chrono_tz::America::Argentina::Buenos_Aires;
    match source {
        AssetSource::Coingecko { .. } => None, // 24/7
        // v1: commodities/indices/rates (CNBC-backed) are not gated — always polled.
        // Commodities trade ~24/5; VIX/yields off-hours polling is cheap (cached). Per-class
        // calendars are a future iteration.
        AssetSource::Commodity { .. } | AssetSource::Index { .. } | AssetSource::Rate { .. } => {
            None
        }
        AssetSource::Data912 { .. } => Some(MarketSpec {
            tz: ba,
            open: at(10, 30),
            fetch_close: at(19, 30), // 17:00 close + ~2h30 feed delay/grace
            // /live/ endpoints carry only currently-trading rows; off-session they empty out.
            closed_cache_mode: ClosedCacheMode::LiveSessionOnly,
        }),
        AssetSource::Dolarapi { .. } => Some(MarketSpec {
            tz: ba,
            open: at(10, 0),
            fetch_close: at(17, 30),
            closed_cache_mode: ClosedCacheMode::LatestSnapshot,
        }),
        AssetSource::Stooq { .. } | AssetSource::Finnhub { .. } | AssetSource::Cnbc { .. } => {
            Some(MarketSpec {
                tz: chrono_tz::America::New_York,
                open: at(9, 30),
                fetch_close: at(16, 15), // 16:00 close + grace
                closed_cache_mode: ClosedCacheMode::LatestSnapshot,
            })
        }
        AssetSource::Frankfurter { .. } => Some(MarketSpec {
            tz: chrono_tz::Europe::Berlin,
            open: at(0, 0),
            fetch_close: at(23, 59), // ECB reference rate: weekday-only, daily
            closed_cache_mode: ClosedCacheMode::LatestSnapshot,
        }),
    }
}

fn is_weekday(wd: Weekday) -> bool {
    !matches!(wd, Weekday::Sat | Weekday::Sun)
}

pub fn gate(source: &AssetSource, now: DateTime<Utc>, cfg: &MarketHours) -> Gate {
    if !cfg.applies_to(source.kind().as_str()) {
        return Gate::Open;
    }
    let spec = match spec(source) {
        None => return Gate::Open,
        Some(s) => s,
    };
    let local = now.with_timezone(&spec.tz);
    let t = local.time();
    let open_now = is_weekday(local.weekday()) && t >= spec.open && t < spec.fetch_close;
    if open_now {
        Gate::Open
    } else {
        let (session_start, last_close) = closed_window(&spec, now);
        Gate::Closed {
            last_close,
            session_start,
            mode: spec.closed_cache_mode,
        }
    }
}

/// The most recent finished weekday session at/before `now`, as `(session_start, last_close)` in
/// UTC: `last_close` is the most recent weekday `fetch_close` instant <= `now`, and `session_start`
/// is that same date's `open`. Both are computed in the market tz.
fn closed_window(spec: &MarketSpec, now: DateTime<Utc>) -> (DateTime<Utc>, DateTime<Utc>) {
    let mut date = now.with_timezone(&spec.tz).date_naive();
    for _ in 0..8 {
        if is_weekday(date.weekday()) {
            if let Some(local_close) = spec
                .tz
                .from_local_datetime(&date.and_time(spec.fetch_close))
                .single()
            {
                let utc_close = local_close.with_timezone(&Utc);
                if utc_close <= now {
                    // DST gap on `open` is vanishingly unlikely at these times; if it ever hits,
                    // clamp session_start to the close so the trust window is empty (fail safe).
                    let session_start = spec
                        .tz
                        .from_local_datetime(&date.and_time(spec.open))
                        .single()
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or(utc_close);
                    return (session_start, utc_close);
                }
            }
        }
        date = match date.pred_opt() {
            Some(d) => d,
            None => break,
        };
    }
    (now, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::model::Panel;

    fn cfg() -> MarketHours {
        MarketHours::default()
    }

    fn coingecko() -> AssetSource {
        AssetSource::Coingecko {
            id: "bitcoin".into(),
            quote: "usd".into(),
        }
    }
    fn cnbc() -> AssetSource {
        AssetSource::Cnbc {
            symbol: "AAPL".into(),
        }
    }
    fn data912() -> AssetSource {
        AssetSource::Data912 {
            panel: Panel::Acciones,
            symbol: "ALUA".into(),
        }
    }
    fn commodity() -> AssetSource {
        AssetSource::Commodity {
            symbol: "gold".into(),
        }
    }

    /// Build a UTC instant from a wall-clock time in the given tz.
    fn utc(tz: Tz, y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        tz.with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn crypto_is_always_open() {
        let now = utc(chrono_tz::UTC, 2026, 6, 7, 3, 0); // a Sunday, 3am
        assert!(matches!(gate(&coingecko(), now, &cfg()), Gate::Open));
    }

    #[test]
    fn a_commodity_is_never_gated_in_v1() {
        // Saturday: equities are closed, but commodities (~24/5) are always polled in v1.
        let ny = chrono_tz::America::New_York;
        let now = utc(ny, 2026, 6, 6, 3, 0);
        assert!(matches!(gate(&commodity(), now, &cfg()), Gate::Open));
    }

    #[test]
    fn gating_disabled_means_open() {
        let mut c = cfg();
        c.enabled = false;
        let now = utc(chrono_tz::America::New_York, 2026, 6, 6, 3, 0); // Sat night
        assert!(matches!(gate(&cnbc(), now, &c), Gate::Open));
    }

    #[test]
    fn byma_is_open_midsession_on_a_weekday() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 4, 14, 0); // Thursday 14:00 ART
        assert!(matches!(gate(&data912(), now, &cfg()), Gate::Open));
    }

    #[test]
    fn byma_is_closed_overnight_with_the_window_on_the_prior_weekday() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 4, 3, 0); // Thursday 03:00 ART (before open)
        match gate(&data912(), now, &cfg()) {
            Gate::Closed {
                last_close,
                session_start,
                mode,
            } => {
                // The window is Wednesday's session: open 10:30, close 19:30 ART.
                let lc = last_close.with_timezone(&ba);
                assert_eq!(lc.weekday(), Weekday::Wed);
                assert_eq!(lc.time(), at(19, 30));
                let ss = session_start.with_timezone(&ba);
                assert_eq!(ss.weekday(), Weekday::Wed);
                assert_eq!(ss.time(), at(10, 30));
                assert_eq!(mode, ClosedCacheMode::LiveSessionOnly);
            }
            Gate::Open => panic!("expected Closed"),
        }
    }

    #[test]
    fn byma_opens_at_1030_and_closes_at_1930_local() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        // Exactly at open -> Open; one minute before -> Closed.
        assert!(matches!(
            gate(&data912(), utc(ba, 2026, 6, 4, 10, 30), &cfg()),
            Gate::Open
        ));
        assert!(matches!(
            gate(&data912(), utc(ba, 2026, 6, 4, 10, 29), &cfg()),
            Gate::Closed { .. }
        ));
        // Exactly at fetch_close -> Closed (t < fetch_close is the open condition).
        assert!(matches!(
            gate(&data912(), utc(ba, 2026, 6, 4, 19, 30), &cfg()),
            Gate::Closed { .. }
        ));
        assert!(matches!(
            gate(&data912(), utc(ba, 2026, 6, 4, 19, 29), &cfg()),
            Gate::Open
        ));
    }

    #[test]
    fn latest_snapshot_providers_use_that_closed_cache_mode() {
        let ny = chrono_tz::America::New_York;
        let now = utc(ny, 2026, 7, 1, 20, 0); // 20:00 ET, after close
        match gate(&cnbc(), now, &cfg()) {
            Gate::Closed { mode, .. } => assert_eq!(mode, ClosedCacheMode::LatestSnapshot),
            Gate::Open => panic!("expected Closed"),
        }
    }

    #[test]
    fn byma_is_closed_on_the_weekend() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 6, 14, 0); // Saturday
        assert!(matches!(gate(&data912(), now, &cfg()), Gate::Closed { .. }));
    }

    #[test]
    fn us_market_open_at_14_local_in_both_dst_seasons() {
        let ny = chrono_tz::America::New_York;
        // Summer (EDT) Wednesday and winter (EST) Wednesday — both 14:00 local are open.
        assert!(matches!(
            gate(&cnbc(), utc(ny, 2026, 7, 1, 14, 0), &cfg()),
            Gate::Open
        ));
        assert!(matches!(
            gate(&cnbc(), utc(ny, 2026, 1, 7, 14, 0), &cfg()),
            Gate::Open
        ));
    }

    #[test]
    fn us_market_closed_after_session() {
        let ny = chrono_tz::America::New_York;
        let now = utc(ny, 2026, 7, 1, 20, 0); // 20:00 ET, after 16:15
        assert!(matches!(gate(&cnbc(), now, &cfg()), Gate::Closed { .. }));
    }

    #[test]
    fn a_provider_can_be_excluded_from_gating() {
        let mut c = cfg();
        c.providers.insert(
            "cnbc".to_string(),
            crate::platform::config::ProviderToggle { enabled: false },
        );
        let ny = chrono_tz::America::New_York;
        let now = utc(ny, 2026, 6, 6, 3, 0); // Sat night
        assert!(matches!(gate(&cnbc(), now, &c), Gate::Open));
    }
}
