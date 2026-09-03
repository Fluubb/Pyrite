//! Typed chat/text components.

use serde::{Deserialize, Serialize};

/// A chat/text component.
///
/// Deliberately minimal: this covers what the MOTD and disconnect reasons need
/// today. It grows when chat lands in a later milestone. Building the MOTD from
/// this struct rather than formatting a JSON string by hand makes malformed
/// output unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextComponent {
    /// The literal text of this component.
    pub text: String,

    /// A named colour, for example `"gold"`, or a `"#rrggbb"` literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Renders bold when `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,

    /// Renders italic when `Some(true)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,

    /// Child components, appended after this one and inheriting its styling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<TextComponent>,
}

impl TextComponent {
    /// Creates an unstyled component carrying only literal text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: None,
            italic: None,
            extra: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn plain_component_serialises_to_just_text() {
        let component = TextComponent::new("A Pyrite Server");
        let json = serde_json::to_string(&component).unwrap();
        assert_eq!(json, r#"{"text":"A Pyrite Server"}"#);
    }

    #[test]
    fn optional_fields_are_omitted_when_unset() {
        let json = serde_json::to_value(TextComponent::new("hi")).unwrap();
        let object = json.as_object().unwrap();
        assert!(!object.contains_key("color"));
        assert!(!object.contains_key("bold"));
        assert!(!object.contains_key("extra"));
    }

    #[test]
    fn styled_component_round_trips() {
        let mut component = TextComponent::new("Pyrite");
        component.color = Some("gold".to_owned());
        component.bold = Some(true);
        component.extra = vec![TextComponent::new(" engine")];

        let json = serde_json::to_string(&component).unwrap();
        let decoded: TextComponent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, component);
    }

    #[test]
    fn deserialises_a_component_with_only_text() {
        let decoded: TextComponent = serde_json::from_str(r#"{"text":"hello"}"#).unwrap();
        assert_eq!(decoded, TextComponent::new("hello"));
    }
}
