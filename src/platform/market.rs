use chrono::{DateTime, Datelike, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

use crate::platform::config::MarketHours;
use crate::platform::model::ProviderKind;

pub enum Gate {
    Open,
    Closed { last_close: DateTime<Utc> },
}

/// Weekday session, with the feed delay + grace already baked into `fetch_close` so we keep
/// polling until the real close has arrived (e.g. data912 is ~2h delayed).
struct MarketSpec {
    tz: Tz,
    open: NaiveTime,
    fetch_close: NaiveTime,
}

fn at(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).expect("valid time")
}

fn spec(kind: ProviderKind) -> Option<MarketSpec> {
    let ba = chrono_tz::America::Argentina::Buenos_Aires;
    match kind {
        ProviderKind::CoinGecko => None, // 24/7
        ProviderKind::Data912 => Some(MarketSpec {
            tz: ba,
            open: at(10, 30),
            fetch_close: at(19, 30), // 17:00 close + ~2h30 feed delay/grace
        }),
        ProviderKind::DolarApi => Some(MarketSpec {
            tz: ba,
            open: at(10, 0),
            fetch_close: at(17, 30),
        }),
        ProviderKind::Stooq | ProviderKind::Finnhub | ProviderKind::Cnbc => Some(MarketSpec {
            tz: chrono_tz::America::New_York,
            open: at(9, 30),
            fetch_close: at(16, 15), // 16:00 close + grace
        }),
        ProviderKind::Frankfurter => Some(MarketSpec {
            tz: chrono_tz::Europe::Berlin,
            open: at(0, 0),
            fetch_close: at(23, 59), // ECB reference rate: weekday-only, daily
        }),
    }
}

fn is_weekday(wd: Weekday) -> bool {
    !matches!(wd, Weekday::Sat | Weekday::Sun)
}

pub fn gate(kind: ProviderKind, now: DateTime<Utc>, cfg: &MarketHours) -> Gate {
    if !cfg.applies_to(kind.as_str()) {
        return Gate::Open;
    }
    let spec = match spec(kind) {
        None => return Gate::Open,
        Some(s) => s,
    };
    let local = now.with_timezone(&spec.tz);
    let t = local.time();
    let open_now = is_weekday(local.weekday()) && t >= spec.open && t < spec.fetch_close;
    if open_now {
        Gate::Open
    } else {
        Gate::Closed {
            last_close: last_close(&spec, now),
        }
    }
}

/// Most recent weekday `fetch_close` instant <= `now`, computed in the market tz, as UTC.
fn last_close(spec: &MarketSpec, now: DateTime<Utc>) -> DateTime<Utc> {
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
                    return utc_close;
                }
            }
        }
        date = match date.pred_opt() {
            Some(d) => d,
            None => break,
        };
    }
    now
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MarketHours {
        MarketHours::default()
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
        assert!(matches!(
            gate(ProviderKind::CoinGecko, now, &cfg()),
            Gate::Open
        ));
    }

    #[test]
    fn gating_disabled_means_open() {
        let mut c = cfg();
        c.enabled = false;
        let now = utc(chrono_tz::America::New_York, 2026, 6, 6, 3, 0); // Sat night
        assert!(matches!(gate(ProviderKind::Cnbc, now, &c), Gate::Open));
    }

    #[test]
    fn byma_is_open_midsession_on_a_weekday() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 4, 14, 0); // Thursday 14:00 ART
        assert!(matches!(
            gate(ProviderKind::Data912, now, &cfg()),
            Gate::Open
        ));
    }

    #[test]
    fn byma_is_closed_overnight_with_last_close_on_the_prior_weekday() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 4, 3, 0); // Thursday 03:00 ART (before open)
        match gate(ProviderKind::Data912, now, &cfg()) {
            Gate::Closed { last_close } => {
                // last close should be Wednesday 19:30 ART
                let lc = last_close.with_timezone(&ba);
                assert_eq!(lc.weekday(), Weekday::Wed);
                assert_eq!(lc.time(), at(19, 30));
            }
            Gate::Open => panic!("expected Closed"),
        }
    }

    #[test]
    fn byma_is_closed_on_the_weekend() {
        let ba = chrono_tz::America::Argentina::Buenos_Aires;
        let now = utc(ba, 2026, 6, 6, 14, 0); // Saturday
        assert!(matches!(
            gate(ProviderKind::Data912, now, &cfg()),
            Gate::Closed { .. }
        ));
    }

    #[test]
    fn us_market_open_at_14_local_in_both_dst_seasons() {
        let ny = chrono_tz::America::New_York;
        // Summer (EDT) Wednesday and winter (EST) Wednesday — both 14:00 local are open.
        assert!(matches!(
            gate(ProviderKind::Cnbc, utc(ny, 2026, 7, 1, 14, 0), &cfg()),
            Gate::Open
        ));
        assert!(matches!(
            gate(ProviderKind::Cnbc, utc(ny, 2026, 1, 7, 14, 0), &cfg()),
            Gate::Open
        ));
    }

    #[test]
    fn us_market_closed_after_session() {
        let ny = chrono_tz::America::New_York;
        let now = utc(ny, 2026, 7, 1, 20, 0); // 20:00 ET, after 16:15
        assert!(matches!(
            gate(ProviderKind::Cnbc, now, &cfg()),
            Gate::Closed { .. }
        ));
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
        assert!(matches!(gate(ProviderKind::Cnbc, now, &c), Gate::Open));
    }
}
