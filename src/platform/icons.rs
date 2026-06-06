use crate::platform::model::Direction;

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
}
