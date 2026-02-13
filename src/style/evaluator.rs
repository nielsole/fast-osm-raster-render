/// MapCSS style evaluator - matches selectors against OSM objects
///
/// Phase 0: Minimal evaluation
/// - Match way selectors against objects
/// - Test tag conditions (equality only)
/// - Return evaluated style properties

use super::types::*;
use std::collections::HashMap;

/// Evaluated style for an object
#[derive(Debug, Clone)]
pub struct EvaluatedStyle {
    pub color: Option<super::Color>,
    pub fill_color: Option<super::Color>,
    pub casing_color: Option<super::Color>,
    pub casing_width: Option<f32>,
    pub width: Option<f32>,
    pub opacity: Option<f32>,
    pub z_index: Option<i32>,
}

impl Default for EvaluatedStyle {
    fn default() -> Self {
        EvaluatedStyle {
            color: Some(super::Color::BLACK), // Default black
            fill_color: None,                 // No fill by default
            casing_color: None,               // No casing by default
            casing_width: None,
            width: Some(1.0),                 // Default 1px
            opacity: Some(1.0),               // Default opaque
            z_index: Some(0),                 // Default layer
        }
    }
}

/// Evaluate a stylesheet against an object's tags
///
/// Returns the evaluated style if any rules match, None otherwise
pub fn evaluate_style(
    stylesheet: &StyleSheet,
    object_type: ObjectType,
    tags: &HashMap<String, String>,
    zoom: u32,
) -> Option<EvaluatedStyle> {
    let mut style = EvaluatedStyle::default();
    let mut matched = false;

    for rule in &stylesheet.rules {
        if matches_selector(&rule.selector, object_type, tags, zoom) {
            matched = true;
            apply_declarations(&mut style, &rule.declarations);
        }
    }

    if matched {
        Some(style)
    } else {
        None
    }
}

/// Evaluate a stylesheet against an object using a tag lookup function.
/// This avoids building a HashMap when tags are read from mmap.
///
/// `tag_lookup` takes a key and returns Some(value) if the tag exists.
/// `has_tag` takes a key and returns true if the tag exists (for Exists checks).
pub fn evaluate_style_with_lookup<F, G>(
    stylesheet: &StyleSheet,
    object_type: ObjectType,
    tag_lookup: F,
    has_tag: G,
    zoom: u32,
) -> Option<EvaluatedStyle>
where
    F: Fn(&str) -> Option<String>,
    G: Fn(&str) -> bool,
{
    let mut style = EvaluatedStyle::default();
    let mut matched = false;

    for rule in &stylesheet.rules {
        if matches_selector_with_lookup(&rule.selector, object_type, &tag_lookup, &has_tag, zoom) {
            matched = true;
            apply_declarations(&mut style, &rule.declarations);
        }
    }

    if matched {
        Some(style)
    } else {
        None
    }
}

/// Check if a selector matches using lookup functions (no HashMap)
fn matches_selector_with_lookup<F, G>(
    selector: &Selector,
    object_type: ObjectType,
    tag_lookup: &F,
    has_tag: &G,
    zoom: u32,
) -> bool
where
    F: Fn(&str) -> Option<String>,
    G: Fn(&str) -> bool,
{
    if !matches_object_type(selector.object_type, object_type) {
        return false;
    }

    if let Some(ref zoom_range) = selector.zoom_range {
        if let Some(min) = zoom_range.min {
            if zoom < min {
                return false;
            }
        }
        if let Some(max) = zoom_range.max {
            if zoom > max {
                return false;
            }
        }
    }

    for condition in &selector.conditions {
        if !matches_tag_test_with_lookup(condition, tag_lookup, has_tag) {
            return false;
        }
    }

    true
}

/// Check if a tag test matches using lookup functions
fn matches_tag_test_with_lookup<F, G>(test: &TagTest, tag_lookup: &F, has_tag: &G) -> bool
where
    F: Fn(&str) -> Option<String>,
    G: Fn(&str) -> bool,
{
    match test.operator {
        CompareOp::Equal => {
            tag_lookup(&test.key).map(|v| v == test.value).unwrap_or(false)
        }
        CompareOp::NotEqual => {
            tag_lookup(&test.key).map(|v| v != test.value).unwrap_or(true)
        }
        CompareOp::Exists => has_tag(&test.key),
        CompareOp::NotExists => !has_tag(&test.key),
        _ => false,
    }
}

/// Check if a selector matches an object
fn matches_selector(
    selector: &Selector,
    object_type: ObjectType,
    tags: &HashMap<String, String>,
    zoom: u32,
) -> bool {
    // Check object type
    if !matches_object_type(selector.object_type, object_type) {
        return false;
    }

    // Check zoom range
    if let Some(ref zoom_range) = selector.zoom_range {
        if let Some(min) = zoom_range.min {
            if zoom < min {
                return false;
            }
        }
        if let Some(max) = zoom_range.max {
            if zoom > max {
                return false;
            }
        }
    }

    // Check all tag conditions
    for condition in &selector.conditions {
        if !matches_tag_test(condition, tags) {
            return false;
        }
    }

    true
}

/// Check if object type matches selector
fn matches_object_type(selector_type: ObjectType, object_type: ObjectType) -> bool {
    match selector_type {
        ObjectType::Any => true,
        _ => selector_type == object_type,
    }
}

/// Check if a tag test matches
fn matches_tag_test(test: &TagTest, tags: &HashMap<String, String>) -> bool {
    match test.operator {
        CompareOp::Equal => {
            tags.get(&test.key).map(|v| v == &test.value).unwrap_or(false)
        }
        CompareOp::NotEqual => {
            tags.get(&test.key).map(|v| v != &test.value).unwrap_or(true)
        }
        CompareOp::Exists => tags.contains_key(&test.key),
        CompareOp::NotExists => !tags.contains_key(&test.key),
        // Phase 1: other operators
        _ => false,
    }
}

/// Apply declarations to a style
fn apply_declarations(style: &mut EvaluatedStyle, declarations: &[Declaration]) {
    for decl in declarations {
        match decl {
            Declaration::Color(color) => style.color = Some(*color),
            Declaration::FillColor(color) => style.fill_color = Some(*color),
            Declaration::CasingColor(color) => style.casing_color = Some(*color),
            Declaration::CasingWidth(width) => style.casing_width = Some(*width),
            Declaration::Width(width) => style.width = Some(*width),
            Declaration::Opacity(opacity) => style.opacity = Some(*opacity),
            Declaration::ZIndex(z) => style.z_index = Some(*z),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    fn make_tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn test_matches_object_type() {
        assert!(matches_object_type(ObjectType::Way, ObjectType::Way));
        assert!(matches_object_type(ObjectType::Any, ObjectType::Way));
        assert!(!matches_object_type(ObjectType::Node, ObjectType::Way));
    }

    #[test]
    fn test_matches_tag_test_equal() {
        let test = TagTest {
            key: "highway".to_string(),
            operator: CompareOp::Equal,
            value: "primary".to_string(),
        };

        let tags = make_tags(&[("highway", "primary")]);
        assert!(matches_tag_test(&test, &tags));

        let tags = make_tags(&[("highway", "secondary")]);
        assert!(!matches_tag_test(&test, &tags));

        let tags = make_tags(&[("building", "yes")]);
        assert!(!matches_tag_test(&test, &tags));
    }

    #[test]
    fn test_matches_tag_test_not_equal() {
        let test = TagTest {
            key: "tunnel".to_string(),
            operator: CompareOp::NotEqual,
            value: "yes".to_string(),
        };

        let tags = make_tags(&[("tunnel", "no")]);
        assert!(matches_tag_test(&test, &tags));

        let tags = make_tags(&[("tunnel", "yes")]);
        assert!(!matches_tag_test(&test, &tags));

        // Tag not present -> matches (!=yes means "not yes")
        let tags = make_tags(&[]);
        assert!(matches_tag_test(&test, &tags));
    }

    #[test]
    fn test_matches_selector() {
        let selector = Selector {
            object_type: ObjectType::Way,
            conditions: vec![TagTest {
                key: "highway".to_string(),
                operator: CompareOp::Equal,
                value: "primary".to_string(),
            }],
            zoom_range: None,
        };

        let tags = make_tags(&[("highway", "primary")]);
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 10));

        let tags = make_tags(&[("highway", "secondary")]);
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 10));

        // Wrong object type
        assert!(!matches_selector(&selector, ObjectType::Node, &tags, 10));
    }

    #[test]
    fn test_zoom_in_range() {
        let selector = Selector {
            object_type: ObjectType::Way,
            conditions: vec![],
            zoom_range: Some(super::super::types::ZoomRange { min: Some(8), max: Some(15) }),
        };

        let tags = make_tags(&[("highway", "primary")]);
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 10));
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 8));
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 15));
    }

    #[test]
    fn test_zoom_below_range() {
        let selector = Selector {
            object_type: ObjectType::Way,
            conditions: vec![],
            zoom_range: Some(super::super::types::ZoomRange { min: Some(10), max: Some(15) }),
        };

        let tags = make_tags(&[("highway", "primary")]);
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 5));
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 9));
    }

    #[test]
    fn test_zoom_above_range() {
        let selector = Selector {
            object_type: ObjectType::Way,
            conditions: vec![],
            zoom_range: Some(super::super::types::ZoomRange { min: Some(10), max: Some(15) }),
        };

        let tags = make_tags(&[("highway", "primary")]);
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 16));
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 20));
    }

    #[test]
    fn test_zoom_min_only() {
        let selector = Selector {
            object_type: ObjectType::Way,
            conditions: vec![],
            zoom_range: Some(super::super::types::ZoomRange { min: Some(12), max: None }),
        };

        let tags = make_tags(&[("highway", "residential")]);
        assert!(!matches_selector(&selector, ObjectType::Way, &tags, 11));
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 12));
        assert!(matches_selector(&selector, ObjectType::Way, &tags, 18));
    }

    #[test]
    fn test_evaluate_style_match() {
        let stylesheet = StyleSheet {
            rules: vec![Rule {
                selector: Selector::way_with_tag("highway", "primary"),
                declarations: vec![Declaration::Color(Color::RED)],
            }],
        };

        let tags = make_tags(&[("highway", "primary")]);
        let style = evaluate_style(&stylesheet, ObjectType::Way, &tags, 10);

        assert!(style.is_some());
        let style = style.unwrap();
        assert_eq!(style.color.unwrap(), Color::RED);
    }

    #[test]
    fn test_evaluate_style_no_match() {
        let stylesheet = StyleSheet {
            rules: vec![Rule {
                selector: Selector::way_with_tag("highway", "primary"),
                declarations: vec![Declaration::Color(Color::RED)],
            }],
        };

        let tags = make_tags(&[("highway", "secondary")]);
        let style = evaluate_style(&stylesheet, ObjectType::Way, &tags, 10);

        assert!(style.is_none());
    }

    #[test]
    fn test_evaluate_multiple_conditions() {
        let stylesheet = StyleSheet {
            rules: vec![Rule {
                selector: Selector {
                    object_type: ObjectType::Way,
                    conditions: vec![
                        TagTest {
                            key: "highway".to_string(),
                            operator: CompareOp::Equal,
                            value: "primary".to_string(),
                        },
                        TagTest {
                            key: "tunnel".to_string(),
                            operator: CompareOp::NotEqual,
                            value: "yes".to_string(),
                        },
                    ],
                    zoom_range: None,
                },
                declarations: vec![Declaration::Color(Color::BLUE)],
            }],
        };

        // Both conditions match
        let tags = make_tags(&[("highway", "primary"), ("tunnel", "no")]);
        let style = evaluate_style(&stylesheet, ObjectType::Way, &tags, 10);
        assert!(style.is_some());

        // First matches, second doesn't
        let tags = make_tags(&[("highway", "primary"), ("tunnel", "yes")]);
        let style = evaluate_style(&stylesheet, ObjectType::Way, &tags, 10);
        assert!(style.is_none());
    }
}
