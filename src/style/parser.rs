/// MapCSS parser using nom combinator library
///
/// Supports:
/// - Multiple rules
/// - Selectors with zoom ranges: way|z10-15[highway=primary]
/// - Declarations: color, width, z-index, opacity

use super::color::{Color, ColorParseError};
use super::types::*;
use nom::{
    branch::alt,
    bytes::complete::{tag, take_while1},
    character::complete::{char, multispace0},
    combinator::{map, opt},
    number::complete::float,
    sequence::{delimited, preceded, tuple},
    IResult,
};

/// Parse a complete MapCSS stylesheet
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

/// Parse tag test: [key=value], [key!=value], or [key] (existence check)
fn tag_test(input: &str) -> IResult<&str, TagTest> {
    // Try key=value / key!=value first, then fall back to key-only existence check
    alt((
        tag_test_with_value,
        tag_test_exists,
    ))(input)
}

/// Parse tag test with operator and value: [key=value] or [key!=value]
fn tag_test_with_value(input: &str) -> IResult<&str, TagTest> {
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

/// Parse tag existence test: [key] (no operator, no value)
fn tag_test_exists(input: &str) -> IResult<&str, TagTest> {
    delimited(
        char('['),
        ws(identifier),
        char(']'),
    )(input)
    .map(|(rest, key)| {
        (
            rest,
            TagTest {
                key: key.to_string(),
                operator: CompareOp::Exists,
                value: String::new(),
            },
        )
    })
}

/// Parse zoom range: |z10-15, |z14, |z12-, |z-8
fn zoom_range(input: &str) -> IResult<&str, ZoomRange> {
    let (rest, _) = tag("|z")(input)?;

    // Parse optional min zoom
    let (rest, min_str) = opt(take_while1(|c: char| c.is_ascii_digit()))(rest)?;
    let min = min_str.map(|s: &str| s.parse::<u32>().unwrap());

    // Check for dash (range separator)
    let (rest, has_dash) = opt(char('-'))(rest)?;

    if has_dash.is_some() {
        // Parse optional max zoom after dash
        let (rest, max_str) = opt(take_while1(|c: char| c.is_ascii_digit()))(rest)?;
        let max = max_str.map(|s: &str| s.parse::<u32>().unwrap());
        Ok((rest, ZoomRange { min, max }))
    } else {
        // Single zoom level: |z14 means min=14, max=14
        Ok((rest, ZoomRange { min, max: min }))
    }
}

/// Parse selector: way|z10-15[highway=primary]
fn selector(input: &str) -> IResult<&str, Selector> {
    map(
        tuple((ws(object_type), opt(zoom_range), nom::multi::many0(ws(tag_test)))),
        |(obj_type, zoom, conditions)| Selector {
            object_type: obj_type,
            conditions,
            zoom_range: zoom,
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

/// Parse a float value (for width, opacity)
fn float_value(input: &str) -> IResult<&str, f32> {
    float(input)
}

/// Parse an integer value (for z-index, supports negative)
fn integer_value(input: &str) -> IResult<&str, i32> {
    let (rest, neg) = opt(char('-'))(input)?;
    let (rest, digits) = take_while1(|c: char| c.is_ascii_digit())(rest)?;
    let val: i32 = digits.parse().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    Ok((rest, if neg.is_some() { -val } else { val }))
}

/// Parse color declaration: color: #ff0000;
fn color_declaration(input: &str) -> IResult<&str, Declaration> {
    let (rest, _) = ws(tag("color"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, color) = ws(hex_color)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;
    Ok((rest, Declaration::Color(color)))
}

/// Parse fill-color declaration: fill-color: #aad3df;
fn fill_color_declaration(input: &str) -> IResult<&str, Declaration> {
    let (rest, _) = ws(tag("fill-color"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, color) = ws(hex_color)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;
    Ok((rest, Declaration::FillColor(color)))
}

/// Parse width declaration: width: 2.5;
fn width_declaration(input: &str) -> IResult<&str, Declaration> {
    let (rest, _) = ws(tag("width"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, value) = ws(float_value)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;
    Ok((rest, Declaration::Width(value)))
}

/// Parse z-index declaration: z-index: 5;
fn z_index_declaration(input: &str) -> IResult<&str, Declaration> {
    let (rest, _) = ws(tag("z-index"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, value) = ws(integer_value)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;
    Ok((rest, Declaration::ZIndex(value)))
}

/// Parse opacity declaration: opacity: 0.5;
fn opacity_declaration(input: &str) -> IResult<&str, Declaration> {
    let (rest, _) = ws(tag("opacity"))(input)?;
    let (rest, _) = ws(char(':'))(rest)?;
    let (rest, value) = ws(float_value)(rest)?;
    let (rest, _) = ws(char(';'))(rest)?;
    Ok((rest, Declaration::Opacity(value)))
}

/// Parse any declaration
fn declaration(input: &str) -> IResult<&str, Declaration> {
    alt((
        fill_color_declaration,
        color_declaration,
        width_declaration,
        z_index_declaration,
        opacity_declaration,
    ))(input)
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

/// Parse stylesheet (multiple rules)
fn stylesheet(input: &str) -> IResult<&str, StyleSheet> {
    map(nom::multi::many1(ws(rule)), |rules| StyleSheet { rules })(input)
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
    fn test_parse_width_declaration() {
        let (_, decl) = declaration("width: 2.5;").unwrap();
        match decl {
            Declaration::Width(w) => assert_eq!(w, 2.5),
            _ => panic!("Expected Width declaration"),
        }

        let (_, decl) = declaration("width: 1;").unwrap();
        match decl {
            Declaration::Width(w) => assert_eq!(w, 1.0),
            _ => panic!("Expected Width declaration"),
        }
    }

    #[test]
    fn test_parse_z_index_declaration() {
        let (_, decl) = declaration("z-index: 5;").unwrap();
        match decl {
            Declaration::ZIndex(z) => assert_eq!(z, 5),
            _ => panic!("Expected ZIndex declaration"),
        }

        let (_, decl) = declaration("z-index: -1;").unwrap();
        match decl {
            Declaration::ZIndex(z) => assert_eq!(z, -1),
            _ => panic!("Expected ZIndex declaration"),
        }
    }

    #[test]
    fn test_parse_opacity_declaration() {
        let (_, decl) = declaration("opacity: 0.5;").unwrap();
        match decl {
            Declaration::Opacity(o) => assert_eq!(o, 0.5),
            _ => panic!("Expected Opacity declaration"),
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

    #[test]
    fn test_parse_zoom_range_full() {
        let (_, zr) = zoom_range("|z10-15").unwrap();
        assert_eq!(zr.min, Some(10));
        assert_eq!(zr.max, Some(15));
    }

    #[test]
    fn test_parse_zoom_range_single() {
        let (_, zr) = zoom_range("|z14").unwrap();
        assert_eq!(zr.min, Some(14));
        assert_eq!(zr.max, Some(14));
    }

    #[test]
    fn test_parse_zoom_range_min_only() {
        let (_, zr) = zoom_range("|z12-").unwrap();
        assert_eq!(zr.min, Some(12));
        assert_eq!(zr.max, None);
    }

    #[test]
    fn test_parse_zoom_range_max_only() {
        let (_, zr) = zoom_range("|z-8").unwrap();
        assert_eq!(zr.min, None);
        assert_eq!(zr.max, Some(8));
    }

    #[test]
    fn test_parse_selector_with_zoom() {
        let (_, sel) = selector("way|z10-15[highway=primary]").unwrap();
        assert_eq!(sel.object_type, ObjectType::Way);
        assert_eq!(sel.zoom_range, Some(ZoomRange { min: Some(10), max: Some(15) }));
        assert_eq!(sel.conditions.len(), 1);
        assert_eq!(sel.conditions[0].key, "highway");
    }

    #[test]
    fn test_parse_selector_zoom_min_only() {
        let (_, sel) = selector("way|z6-[highway=motorway]").unwrap();
        assert_eq!(sel.zoom_range, Some(ZoomRange { min: Some(6), max: None }));
        assert_eq!(sel.conditions[0].value, "motorway");
    }

    #[test]
    fn test_parse_multiple_declarations() {
        let input = "way[highway=primary] { color: #ff0000; width: 3; z-index: 7; }";
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules[0].declarations.len(), 3);
    }

    #[test]
    fn test_parse_multiple_rules() {
        let input = "
            way[highway=motorway] { color: #e892a2; width: 5; z-index: 9; }
            way[highway=primary] { color: #fcd6a4; width: 3; z-index: 7; }
            way { color: #cccccc; width: 0.5; z-index: 0; }
        ";
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules.len(), 3);
        assert_eq!(stylesheet.rules[0].selector.conditions[0].value, "motorway");
        assert_eq!(stylesheet.rules[1].selector.conditions[0].value, "primary");
        assert_eq!(stylesheet.rules[2].selector.conditions.len(), 0); // catch-all
    }

    #[test]
    fn test_parse_fill_color_declaration() {
        let (_, decl) = declaration("fill-color: #aad3df;").unwrap();
        match decl {
            Declaration::FillColor(c) => {
                // #aad3df = (170, 211, 223)
                assert!((c.r - 170.0 / 255.0).abs() < 0.01);
                assert!((c.g - 211.0 / 255.0).abs() < 0.01);
                assert!((c.b - 223.0 / 255.0).abs() < 0.01);
            }
            _ => panic!("Expected FillColor declaration"),
        }
    }

    #[test]
    fn test_parse_tag_exists() {
        let (_, test) = tag_test("[building]").unwrap();
        assert_eq!(test.key, "building");
        assert_eq!(test.operator, CompareOp::Exists);
        assert_eq!(test.value, "");
    }

    #[test]
    fn test_parse_area_rule_with_fill_color() {
        let input = "area[building] { fill-color: #d9d0c9; z-index: 1; }";
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules.len(), 1);
        assert_eq!(stylesheet.rules[0].selector.object_type, ObjectType::Area);
        assert_eq!(stylesheet.rules[0].selector.conditions[0].key, "building");
        assert_eq!(stylesheet.rules[0].selector.conditions[0].operator, CompareOp::Exists);
        assert_eq!(stylesheet.rules[0].declarations.len(), 2);
        match &stylesheet.rules[0].declarations[0] {
            Declaration::FillColor(_) => {}
            other => panic!("Expected FillColor, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_full_road_hierarchy() {
        let input = r#"
            way|z6-[highway=motorway] { color: #e892a2; width: 5; z-index: 9; }
            way|z8-[highway=trunk] { color: #f9b29c; width: 4; z-index: 8; }
            way|z8-[highway=primary] { color: #fcd6a4; width: 3; z-index: 7; }
            way { color: #cccccc; width: 0.5; z-index: 0; }
        "#;
        let stylesheet = parse_mapcss(input).unwrap();
        assert_eq!(stylesheet.rules.len(), 4);

        // Check zoom ranges
        assert_eq!(stylesheet.rules[0].selector.zoom_range, Some(ZoomRange { min: Some(6), max: None }));
        assert_eq!(stylesheet.rules[1].selector.zoom_range, Some(ZoomRange { min: Some(8), max: None }));
        assert_eq!(stylesheet.rules[3].selector.zoom_range, None); // catch-all
    }
}
