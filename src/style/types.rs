/// MapCSS AST (Abstract Syntax Tree) types
///
/// Phase 0: Minimal types for proof-of-concept
/// - Single rule support
/// - Basic tag selector (way[highway=primary])
/// - Color declaration only

use super::color::Color;

/// A complete MapCSS stylesheet
#[derive(Debug, Clone, PartialEq)]
pub struct StyleSheet {
    pub rules: Vec<Rule>,
}

/// A MapCSS rule: selector { declarations }
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selector: Selector,
    pub declarations: Vec<Declaration>,
}

/// Selector for matching OSM objects
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    /// Object type (way, node, area, line, etc.)
    pub object_type: ObjectType,
    /// Tag conditions ([highway=primary])
    pub conditions: Vec<TagTest>,
    /// Zoom level range (|z10-15) - Phase 1
    pub zoom_range: Option<ZoomRange>,
}

/// OSM object types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Way,
    Node,
    Area,
    Line,
    Canvas,
    Any, // wildcard *
}

/// Tag test condition
#[derive(Debug, Clone, PartialEq)]
pub struct TagTest {
    pub key: String,
    pub operator: CompareOp,
    pub value: String,
}

/// Comparison operators for tag tests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Equal,       // =
    NotEqual,    // !=
    LessThan,    // < (Phase 1)
    GreaterThan, // > (Phase 1)
    Regex,       // =~ (Phase 1)
    Exists,      // [highway] - no value (Phase 1)
    NotExists,   // [!highway] (Phase 1)
}

/// Zoom range for selector
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoomRange {
    pub min: Option<u32>,
    pub max: Option<u32>,
}

/// Style declaration (property: value)
#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Color(Color),
    FillColor(Color),
    Width(f32),
    Opacity(f32),
    ZIndex(i32),
}

impl Selector {
    /// Create a simple way selector with single tag test
    pub fn way_with_tag(key: &str, value: &str) -> Self {
        Selector {
            object_type: ObjectType::Way,
            conditions: vec![TagTest {
                key: key.to_string(),
                operator: CompareOp::Equal,
                value: value.to_string(),
            }],
            zoom_range: None,
        }
    }
}

impl Rule {
    /// Create a simple rule: selector { color: value }
    pub fn simple_color_rule(selector: Selector, color: Color) -> Self {
        Rule {
            selector,
            declarations: vec![Declaration::Color(color)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_creation() {
        let sel = Selector::way_with_tag("highway", "primary");
        assert_eq!(sel.object_type, ObjectType::Way);
        assert_eq!(sel.conditions.len(), 1);
        assert_eq!(sel.conditions[0].key, "highway");
        assert_eq!(sel.conditions[0].value, "primary");
    }

    #[test]
    fn test_rule_creation() {
        let sel = Selector::way_with_tag("highway", "primary");
        let color = Color::from_hex("#ff0000").unwrap();
        let rule = Rule::simple_color_rule(sel, color);

        assert_eq!(rule.declarations.len(), 1);
        match &rule.declarations[0] {
            Declaration::Color(c) => assert_eq!(c, &color),
            _ => panic!("Expected Color declaration"),
        }
    }
}
