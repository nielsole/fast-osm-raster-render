/// MapCSS parser using nom combinator library
///
/// Phase 0: Minimal parser for proof-of-concept
/// - Parses: way[highway=primary] { color: #ff0000; }
/// - Single rule only
/// - Basic tag selector

use super::color::{Color, ColorParseError};
use super::types::*;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{char, multispace0},
    combinator::map,
    sequence::{delimited, preceded, tuple},
    IResult,
};

/// Parse a complete MapCSS stylesheet (Phase 0: single rule only)
pub fn parse_mapcss(input: &str) -> Result<StyleSheet, ParseError> {
    match stylesheet(input) {
        Ok((_, stylesheet)) => Ok(stylesheet),
        Err(e) => Err(ParseError::NomError(format!("{:?}", e))),
    }
}

/// Parse error type
#[derive(Debug, Clone)]
pub enum ParseError {
    NomError(String),
    ColorError(ColorParseError),
    InvalidSyntax(String),
}

impl From<ColorParseError> for ParseError {
    fn from(e: ColorParseError) -> Self {
        ParseError::ColorError(e)
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NomError(s) => write!(f, "Parse error: {}", s),
            ParseError::ColorError(e) => write!(f, "Color error: {}", e),
            ParseError::InvalidSyntax(s) => write!(f, "Invalid syntax: {}", s),
        }
    }
}

impl std::error::Error for ParseError {}

// Nom parsers

/// Parse whitespace
fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    delimited(multispace0, inner, multispace0)
}

/// Parse identifier (letters, digits, underscore, hyphen)
fn identifier(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c.is_alphanumeric() || c == '_' || c == '-')(input)
}

/// Parse object type: way, node, area, line, *, etc.
fn object_type(input: &str) -> IResult<&str, ObjectType> {
    alt((
        map(tag("way"), |_| ObjectType::Way),
        map(tag("node"), |_| ObjectType::Node),
        map(tag("area"), |_| ObjectType::Area),
        map(tag("line"), |_| ObjectType::Line),
        map(tag("canvas"), |_| ObjectType::Canvas),
        map(char('*'), |_| ObjectType::Any),
    ))(input)
}

/// Parse tag test: [key=value] or [key!=value]
fn tag_test(input: &str) -> IResult<&str, TagTest> {
    delimited(
        char('['),
        ws(tuple((
            identifier,
            ws(alt((
                map(tag("!="), |_| CompareOp::NotEqual),
                map(char('='), |_| CompareOp::Equal),
            ))),
            identifier,
        ))),
        char(']'),
    )(input)
    .map(|(rest, (key, op, value))| {
        (
            rest,
            TagTest {
                key: key.to_string(),
                operator: op,
                value: value.to_string(),
            },
        )
    })
}

/// Parse selector: way[highway=primary]
fn selector(input: &str) -> IResult<&str, Selector> {
    map(
        tuple((ws(object_type), nom::multi::many0(ws(tag_test)))),
        |(obj_type, conditions)| Selector {
            object_type: obj_type,
            conditions,
            zoom_range: None, // Phase 1
        },
    )(input)
}

/// Parse hex color value: #ff0000
fn hex_color(input: &str) -> IResult<&str, Color> {
    preceded(
        char('#'),
        take_while1(|c: char| c.is_ascii_hexdigit()),
    )(input)
    .and_then(|(rest, hex)| {
        let color_str = format!("#{}", hex);
        match Color::from_hex(&color_str) {
            Ok(color) => Ok((rest, color)),
            Err(_) => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::HexDigit,
            ))),
        }
    })
}

/// Parse declaration: color: #ff0000;
fn declaration(input: &str) -> IResult<&str, Declaration> {
    // Phase 0: only color property
    let (rest, _) = ws(tag("color"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, color) = ws(hex_color)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;

    Ok((rest, Declaration::Color(color)))
}

/// Parse declaration block: { color: #ff0000; }
fn declaration_block(input: &str) -> IResult<&str, Vec<Declaration>> {
    delimited(
        ws(char('{')),
        nom::multi::many0(ws(declaration)),
        ws(char('}')),
    )(input)
}

/// Parse a single rule: selector { declarations }
fn rule(input: &str) -> IResult<&str, Rule> {
    map(
        tuple((ws(selector), ws(declaration_block))),
        |(sel, decls)| Rule {
            selector: sel,
            declarations: decls,
        },
    )(input)
}

/// Parse stylesheet (Phase 0: single rule)
fn stylesheet(input: &str) -> IResult<&str, StyleSheet> {
    map(ws(rule), |r| StyleSheet { rules: vec![r] })(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_identifier() {
        assert_eq!(identifier("highway"), Ok(("", "highway")));
        assert_eq!(identifier("primary"), Ok(("", "primary")));
        assert_eq!(identifier("motorway_link"), Ok(("", "motorway_link")));
    }

    #[test]
    fn test_parse_object_type() {
        assert_eq!(object_type("way"), Ok(("", ObjectType::Way)));
        assert_eq!(object_type("node"), Ok(("", ObjectType::Node)));
        assert_eq!(object_type("*"), Ok(("", ObjectType::Any)));
    }

    #[test]
    fn test_parse_tag_test() {
        let (_, test) = tag_test("[highway=primary]").unwrap();
        assert_eq!(test.key, "highway");
        assert_eq!(test.operator, CompareOp::Equal);
        assert_eq!(test.value, "primary");

        let (_, test) = tag_test("[tunnel!=yes]").unwrap();
        assert_eq!(test.key, "tunnel");
        assert_eq!(test.operator, CompareOp::NotEqual);
        assert_eq!(test.value, "yes");
    }

    #[test]
    fn test_parse_selector() {
        let (_, sel) = selector("way[highway=primary]").unwrap();
        assert_eq!(sel.object_type, ObjectType::Way);
        assert_eq!(sel.conditions.len(), 1);
        assert_eq!(sel.conditions[0].key, "highway");
        assert_eq!(sel.conditions[0].value, "primary");
    }

    #[test]
    fn test_parse_hex_color() {
        let (_, color) = hex_color("#ff0000").unwrap();
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
    }

    #[test]
    fn test_parse_declaration() {
        let (_, decl) = declaration("color: #ff0000;").unwrap();
        match decl {
            Declaration::Color(c) => {
                assert_eq!(c.r, 1.0);
                assert_eq!(c.g, 0.0);
                assert_eq!(c.b, 0.0);
            }
            _ => panic!("Expected Color declaration"),
        }
    }

    #[test]
    fn test_parse_rule() {
        let input = "way[highway=primary] { color: #ff0000; }";
        let (_, r) = rule(input).unwrap();

        assert_eq!(r.selector.object_type, ObjectType::Way);
        assert_eq!(r.selector.conditions.len(), 1);
        assert_eq!(r.declarations.len(), 1);

        match &r.declarations[0] {
            Declaration::Color(c) => {
                assert_eq!(c.r, 1.0);
                assert_eq!(c.g, 0.0);
                assert_eq!(c.b, 0.0);
            }
            _ => panic!("Expected Color declaration"),
        }
    }

    #[test]
    fn test_parse_mapcss() {
        let input = "way[highway=primary] { color: #ff0000; }";
        let stylesheet = parse_mapcss(input).unwrap();

        assert_eq!(stylesheet.rules.len(), 1);
        assert_eq!(stylesheet.rules[0].selector.object_type, ObjectType::Way);
        assert_eq!(stylesheet.rules[0].declarations.len(), 1);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let input = "
            way[highway=primary] {
                color: #ff0000;
            }
        ";
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules.len(), 1);
    }

    #[test]
    fn test_parse_multiple_conditions() {
        let input = "way[highway=primary][tunnel=yes] { color: #ff0000; }";
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules[0].selector.conditions.len(), 2);
    }
}
