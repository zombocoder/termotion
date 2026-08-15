use thiserror::Error;

/// A point in, or span of, virtual time. Nanosecond resolution.
///
/// Frame scheduling uses integer arithmetic exclusively so that timing is
/// identical on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time(u64);

impl Time {
    pub const ZERO: Time = Time(0);

    pub const fn from_nanos(n: u64) -> Self {
        Time(n)
    }

    pub const fn from_millis(ms: u64) -> Self {
        Time(ms.saturating_mul(1_000_000))
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000_000
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000_000.0
    }

    #[must_use]
    pub const fn saturating_add(self, other: Time) -> Time {
        Time(self.0.saturating_add(other.0))
    }

    #[must_use]
    pub const fn saturating_sub(self, other: Time) -> Time {
        Time(self.0.saturating_sub(other.0))
    }

    #[must_use]
    pub const fn saturating_mul(self, factor: u64) -> Time {
        Time(self.0.saturating_mul(factor))
    }
}

/// Frame rate as an exact rational, so 30000/1001 is representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fps {
    num: u32,
    den: u32,
}

impl Fps {
    /// Returns `None` if either term is zero.
    pub const fn new(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            None
        } else {
            Some(Fps { num, den })
        }
    }

    pub const fn from_integer(fps: u32) -> Self {
        // A zero here would be a programming error, not user input; clamp to 1.
        let num = if fps == 0 { 1 } else { fps };
        Fps { num, den: 1 }
    }

    pub const fn num(self) -> u32 {
        self.num
    }

    pub const fn den(self) -> u32 {
        self.den
    }

    /// Exact timestamp of frame `n`: `n * 1e9 * den / num`, truncated.
    pub const fn frame_time(self, n: u64) -> Time {
        let scaled = (n as u128) * 1_000_000_000u128 * (self.den as u128) / (self.num as u128);
        Time(scaled as u64)
    }

    /// Number of frames needed to cover `duration`, rounding up.
    pub const fn frame_count(self, duration: Time) -> u64 {
        let num = (duration.as_nanos() as u128) * (self.num as u128);
        let den = 1_000_000_000u128 * (self.den as u128);
        num.div_ceil(den) as u64
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DurationParseError {
    #[error("duration is empty")]
    Empty,
    #[error("duration `{0}` is missing a unit (expected ms, s, or m)")]
    MissingUnit(String),
    #[error("duration unit `{0}` is not one of ms, s, m")]
    UnknownUnit(String),
    #[error("`{0}` is not a valid number")]
    InvalidNumber(String),
    #[error("duration `{0}` must not be negative")]
    Negative(String),
}

/// Nanoseconds in one millisecond, used to convert a parsed `ms` duration.
const NANOS_PER_MILLI: f64 = 1_000_000.0;

/// Nanoseconds in one second, used to convert a parsed `s` duration.
const NANOS_PER_SEC: f64 = 1_000_000_000.0;

/// Nanoseconds in one minute, used to convert a parsed `m` duration.
const NANOS_PER_MINUTE: f64 = 60_000_000_000.0;

/// Parses `100ms`, `500ms`, `1s`, `2.5s`, `1m`.
///
/// This is the one place `f64` is permitted: a deterministic one-shot
/// conversion into integer nanoseconds, performed at parse time only.
pub fn parse_duration(input: &str) -> Result<Time, DurationParseError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(DurationParseError::Empty);
    }

    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| DurationParseError::MissingUnit(text.to_string()))?;
    let (number, unit) = text.split_at(split);

    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| DurationParseError::InvalidNumber(number.trim().to_string()))?;

    if !value.is_finite() {
        return Err(DurationParseError::InvalidNumber(number.trim().to_string()));
    }
    if value < 0.0 {
        return Err(DurationParseError::Negative(text.to_string()));
    }

    let nanos_per_unit: f64 = match unit.trim() {
        "ms" => NANOS_PER_MILLI,
        "s" => NANOS_PER_SEC,
        "m" => NANOS_PER_MINUTE,
        other => return Err(DurationParseError::UnknownUnit(other.to_string())),
    };

    Ok(Time::from_nanos((value * nanos_per_unit).round() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_documented_units() {
        assert_eq!(parse_duration("100ms").unwrap(), Time::from_millis(100));
        assert_eq!(parse_duration("500ms").unwrap(), Time::from_millis(500));
        assert_eq!(parse_duration("1s").unwrap(), Time::from_millis(1_000));
        assert_eq!(parse_duration("2.5s").unwrap(), Time::from_millis(2_500));
        assert_eq!(parse_duration("1m").unwrap(), Time::from_millis(60_000));
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_eq!(parse_duration("  250ms  ").unwrap(), Time::from_millis(250));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(parse_duration(""), Err(DurationParseError::Empty)));
        assert!(matches!(
            parse_duration("500"),
            Err(DurationParseError::MissingUnit(_))
        ));
        assert!(matches!(
            parse_duration("500h"),
            Err(DurationParseError::UnknownUnit(_))
        ));
        assert!(matches!(
            parse_duration("abcms"),
            Err(DurationParseError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_duration("-5s"),
            Err(DurationParseError::Negative(_))
        ));
    }

    #[test]
    fn frame_timestamps_are_exact_integers() {
        let fps = Fps::from_integer(30);
        assert_eq!(fps.frame_time(0), Time::from_nanos(0));
        assert_eq!(fps.frame_time(1), Time::from_nanos(33_333_333));
        assert_eq!(fps.frame_time(30), Time::from_nanos(1_000_000_000));
        assert_eq!(fps.frame_time(90), Time::from_nanos(3_000_000_000));
    }

    #[test]
    fn supports_ntsc_rational_rates() {
        let fps = Fps::new(30_000, 1_001).unwrap();
        assert_eq!(fps.frame_time(1), Time::from_nanos(33_366_666));
    }

    #[test]
    fn frame_count_covers_the_whole_duration() {
        let fps = Fps::from_integer(30);
        assert_eq!(fps.frame_count(Time::from_millis(1_000)), 30);
        assert_eq!(fps.frame_count(Time::from_millis(1_001)), 31);
        assert_eq!(fps.frame_count(Time::ZERO), 0);
    }

    #[test]
    fn rejects_zero_fps() {
        assert!(Fps::new(0, 1).is_none());
        assert!(Fps::new(30, 0).is_none());
    }
}
