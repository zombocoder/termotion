use thiserror::Error;

/// Straight (non-premultiplied) 8-bit RGBA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ColorParseError {
    #[error("color `{0}` must start with `#`")]
    MissingHash(String),
    #[error("color `{0}` must have 3, 6, or 8 hex digits")]
    BadLength(String),
    #[error("color `{0}` contains a non-hexadecimal digit")]
    BadDigit(String),
}

/// Parses `#RGB`, `#RRGGBB`, and `#RRGGBBAA`.
pub fn parse_color(input: &str) -> Result<Color, ColorParseError> {
    let text = input.trim();
    let body = text
        .strip_prefix('#')
        .ok_or_else(|| ColorParseError::MissingHash(text.to_string()))?;

    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ColorParseError::BadDigit(text.to_string()));
    }

    let nybble = |c: u8| -> u8 { (c as char).to_digit(16).unwrap_or(0) as u8 };
    let bytes = body.as_bytes();

    match bytes.len() {
        3 => {
            let expand = |c: u8| nybble(c) * 17;
            Ok(Color::rgb(
                expand(bytes[0]),
                expand(bytes[1]),
                expand(bytes[2]),
            ))
        }
        6 | 8 => {
            let pair = |i: usize| nybble(bytes[i]) * 16 + nybble(bytes[i + 1]);
            let a = if bytes.len() == 8 { pair(6) } else { 255 };
            Ok(Color::rgba(pair(0), pair(2), pair(4), a))
        }
        _ => Err(ColorParseError::BadLength(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(
            parse_color("#080b09").unwrap(),
            Color {
                r: 8,
                g: 11,
                b: 9,
                a: 255
            }
        );
        assert_eq!(
            parse_color("#39FF14").unwrap(),
            Color {
                r: 57,
                g: 255,
                b: 20,
                a: 255
            }
        );
    }

    #[test]
    fn parses_three_digit_shorthand() {
        assert_eq!(
            parse_color("#0f8").unwrap(),
            Color {
                r: 0,
                g: 255,
                b: 136,
                a: 255
            }
        );
    }

    #[test]
    fn parses_eight_digit_hex_with_alpha() {
        assert_eq!(
            parse_color("#39FF1480").unwrap(),
            Color {
                r: 57,
                g: 255,
                b: 20,
                a: 128
            }
        );
    }

    #[test]
    fn is_case_insensitive_and_trims() {
        assert_eq!(
            parse_color("  #abcdef  ").unwrap(),
            parse_color("#ABCDEF").unwrap()
        );
    }

    #[test]
    fn rejects_malformed_colors() {
        assert!(matches!(
            parse_color("080b09"),
            Err(ColorParseError::MissingHash(_))
        ));
        assert!(matches!(
            parse_color("#0806"),
            Err(ColorParseError::BadLength(_))
        ));
        assert!(matches!(
            parse_color("#gggggg"),
            Err(ColorParseError::BadDigit(_))
        ));
        assert!(matches!(
            parse_color(""),
            Err(ColorParseError::MissingHash(_))
        ));
    }
}
