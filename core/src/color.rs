use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl fmt::Display for RgbColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected an opaque RGB color (#RRGGBB, rgb(r,g,b), or a color name)")]
pub struct ColorParseError;

impl FromStr for RgbColor {
    type Err = ColorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        let hex = value.starts_with('#') && matches!(value.len(), 4 | 7);
        let rgb = value
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgb("));
        let name = value
            .chars()
            .all(|character| character.is_ascii_alphabetic());
        if !(hex || rgb || name) {
            return Err(ColorParseError);
        }

        let [red, green, blue, alpha] = csscolorparser::parse(value)
            .map_err(|_| ColorParseError)?
            .to_rgba8();
        if alpha != 255 {
            return Err(ColorParseError);
        }
        Ok(Self { red, green, blue })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_rgb_and_common_color_names() {
        let red = RgbColor {
            red: 255,
            green: 0,
            blue: 0,
        };
        assert_eq!("#ff0000".parse(), Ok(red));
        assert_eq!("rgb(255, 0, 0)".parse(), Ok(red));
        assert_eq!("red".parse(), Ok(red));
        assert_eq!(
            "rebeccapurple".parse(),
            Ok(RgbColor {
                red: 102,
                green: 51,
                blue: 153
            })
        );
        assert_eq!(RgbColor::default().to_string(), "#000000");
        assert!("rgba(255, 0, 0, 0.5)".parse::<RgbColor>().is_err());
        assert!("notacolor".parse::<RgbColor>().is_err());
    }
}
