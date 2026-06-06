use crate::platform::model::{Direction, ProviderKind};

#[derive(Clone, Copy)]
pub enum IconSet {
    Nerd,
    Emoji,
    Ascii,
}

impl IconSet {
    pub fn from_name(s: &str) -> Self {
        match s {
            "emoji" => IconSet::Emoji,
            "ascii" => IconSet::Ascii,
            _ => IconSet::Nerd,
        }
    }

    /// Directional arrow used in bar text (and never in measured tooltip columns).
    pub fn arrow(&self, d: Option<Direction>) -> &'static str {
        match (self, d) {
            (IconSet::Ascii, Some(Direction::Up)) => "^",
            (IconSet::Ascii, Some(Direction::Down)) => "v",
            (IconSet::Ascii, _) => "=",
            (IconSet::Emoji, Some(Direction::Up)) => "📈",
            (IconSet::Emoji, Some(Direction::Down)) => "📉",
            (IconSet::Emoji, _) => "➖",
            (_, Some(Direction::Up)) => "\u{f062}", // nf-fa-arrow_up
            (_, Some(Direction::Down)) => "\u{f063}", // nf-fa-arrow_down
            (_, _) => "\u{f068}",                   // nf-fa-minus
        }
    }

    /// Per-asset-class glyph for tooltip sub-headers (outside measured columns).
    pub fn kind_glyph(&self, k: ProviderKind) -> &'static str {
        match (self, k) {
            (IconSet::Ascii, _) => "",
            (IconSet::Emoji, ProviderKind::CoinGecko) => "🪙",
            (IconSet::Emoji, ProviderKind::DolarApi) => "💵",
            (IconSet::Emoji, ProviderKind::Stooq) => "📊",
            (IconSet::Emoji, ProviderKind::Frankfurter) => "💱",
            (IconSet::Emoji, ProviderKind::Finnhub) => "📊",
            (IconSet::Emoji, ProviderKind::Cnbc) => "📊",
            (IconSet::Emoji, ProviderKind::Data912) => "📊",
            (_, ProviderKind::CoinGecko) => "\u{f15a}", // nf-fa-bitcoin
            (_, ProviderKind::DolarApi) => "\u{f155}",  // nf-fa-dollar
            (_, ProviderKind::Stooq) => "\u{f201}",     // nf-fa-line_chart
            (_, ProviderKind::Frankfurter) => "\u{f0ec}", // nf-fa-exchange
            (_, ProviderKind::Finnhub) => "\u{f201}",   // nf-fa-line_chart
            (_, ProviderKind::Cnbc) => "\u{f201}",      // nf-fa-line_chart
            (_, ProviderKind::Data912) => "\u{f201}",   // nf-fa-line_chart
        }
    }
}
