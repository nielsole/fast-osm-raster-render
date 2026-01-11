/// Color parsing and representation for MapCSS
///
/// Supports:
/// - Hex colors: #RGB, #RRGGBB, #RRGGBBAA
/// - RGB/RGBA functions: rgb(255, 0, 0), rgba(255, 0, 0, 0.5)
/// - Named colors: red, blue, green, etc. (Phase 1)

use std::fmt;

/// RGBA color (0.0-1.0 range for GPU)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Create color from RGBA components (0.0-1.0)
    pub fn from_rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// Create color from RGB (alpha = 1.0)
    pub fn from_rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    /// Create color from 8-bit RGBA (0-255)
    pub fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Create color from 8-bit RGB (alpha = 255)
    pub fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba_u8(r, g, b, 255)
    }

    /// Parse hex color: #RGB, #RRGGBB, #RRGGBBAA
    pub fn from_hex(hex: &str) -> Result<Self, ColorParseError> {
        let hex = hex.trim_start_matches('#');

        match hex.len() {
            3 => {
                // #RGB -> #RRGGBB
                let r = u8::from_str_radix(&hex[0..1], 16)? * 17; // F -> FF
                let g = u8::from_str_radix(&hex[1..2], 16)? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16)? * 17;
                Ok(Self::from_rgb_u8(r, g, b))
            }
            6 => {
                // #RRGGBB
                let r = u8::from_str_radix(&hex[0..2], 16)?;
                let g = u8::from_str_radix(&hex[2..4], 16)?;
                let b = u8::from_str_radix(&hex[4..6], 16)?;
                Ok(Self::from_rgb_u8(r, g, b))
            }
            8 => {
                // #RRGGBBAA
                let r = u8::from_str_radix(&hex[0..2], 16)?;
                let g = u8::from_str_radix(&hex[2..4], 16)?;
                let b = u8::from_str_radix(&hex[4..6], 16)?;
                let a = u8::from_str_radix(&hex[6..8], 16)?;
                Ok(Self::from_rgba_u8(r, g, b, a))
            }
            _ => Err(ColorParseError::InvalidFormat),
        }
    }

    /// Convert to array for GPU uniform [r, g, b, a]
    pub fn to_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Common colors
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const RED: Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE: Color = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorParseError {
    InvalidFormat,
    ParseError(std::num::ParseIntError),
}

impl From<std::num::ParseIntError> for ColorParseError {
    fn from(e: std::num::ParseIntError) -> Self {
        ColorParseError::ParseError(e)
    }
}

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColorParseError::InvalidFormat => write!(f, "Invalid color format"),
            ColorParseError::ParseError(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl std::error::Error for ColorParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_rgb_u8() {
        let color = Color::from_rgb_u8(255, 0, 0);
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_hex_rgb() {
        let color = Color::from_hex("#ff0000").unwrap();
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_hex_short() {
        let color = Color::from_hex("#f00").unwrap();
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
    }

    #[test]
    fn test_hex_rgba() {
        let color = Color::from_hex("#ff000080").unwrap();
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert!((color.a - 0.502).abs() < 0.01); // 128/255 ≈ 0.502
    }

    #[test]
    fn test_to_array() {
        let color = Color::from_rgb(1.0, 0.5, 0.0);
        let arr = color.to_array();
        assert_eq!(arr, [1.0, 0.5, 0.0, 1.0]);
    }

    #[test]
    fn test_invalid_hex() {
        assert!(Color::from_hex("#ff00").is_err()); // Wrong length
        assert!(Color::from_hex("#gggggg").is_err()); // Invalid hex
    }
}
