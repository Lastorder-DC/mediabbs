//! Navigation menus edited from the admin UI. The structure mirrors the client's
//! menu model (mediabbs-client `src/menu.ts`); it is validated here so that only
//! well-formed menus with safe links are stored.

use std::collections::BTreeMap;

const MAX_ITEMS: usize = 50;
const MAX_TEXT: usize = 100;
const MAX_URL: usize = 2000;

/// A text in one language, or per language (`{"ko": "도움말", "en": "Help"}`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum MenuText {
    Plain(String),
    PerLanguage(BTreeMap<String, String>),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinType {
    RecentChanges,
    Random,
    Boards,
    Tools,
    Wiki,
    NavLinks,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BuiltinItem {
    #[serde(rename = "type")]
    pub kind: BuiltinType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<MenuText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MenuChild {
    pub label: MenuText,
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CustomItem {
    pub label: MenuText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<MenuChild>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum MenuItem {
    Builtin(BuiltinItem),
    Custom(CustomItem),
}

/// Menus per skin name, or `default` for every skin without its own.
pub type Menus = BTreeMap<String, Vec<MenuItem>>;

fn check_text(text: &MenuText, what: &str) -> Result<(), String> {
    let ok = |s: &str| !s.trim().is_empty() && s.chars().count() <= MAX_TEXT;
    match text {
        MenuText::Plain(s) if ok(s) => Ok(()),
        MenuText::PerLanguage(map)
            if !map.is_empty()
                && map.len() <= 20
                && map.iter().all(|(lang, s)| is_key(lang) && ok(s)) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "{} must be 1-{} characters (or a non-empty map of language code to text)",
            what, MAX_TEXT
        )),
    }
}

/// Links must be http(s) URLs or site paths: no `javascript:` and the like.
/// No backslashes: browsers read `/\host` as `//host` (another site).
fn check_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    let ok = url.len() <= MAX_URL
        && !url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        && (lower.starts_with("https://")
            || lower.starts_with("http://")
            || (url.starts_with('/') && !url.starts_with("//")));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "invalid link '{}': use an http(s) URL or a path starting with /",
            url
        ))
    }
}

fn check_icon(icon: &Option<String>) -> Result<(), String> {
    match icon {
        Some(i) if !crate::models::Board::is_valid_icon(i) => {
            Err(format!("invalid icon name '{}'", i))
        }
        _ => Ok(()),
    }
}

/// Skin names and language codes: short lower-case identifiers.
fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 32
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Checks a menu configuration; the error is a message for the admin.
pub fn validate(menus: &Menus) -> Result<(), String> {
    if menus.len() > 20 {
        return Err("too many menus".into());
    }
    for (skin, items) in menus {
        if !is_key(skin) {
            return Err(format!("invalid skin name '{}'", skin));
        }
        if items.len() > MAX_ITEMS {
            return Err(format!("menu '{}' has more than {} items", skin, MAX_ITEMS));
        }
        for item in items {
            match item {
                MenuItem::Builtin(b) => {
                    if let Some(label) = &b.label {
                        check_text(label, "label")?;
                    }
                    check_icon(&b.icon)?;
                }
                MenuItem::Custom(c) => {
                    check_text(&c.label, "label")?;
                    check_icon(&c.icon)?;
                    let children = c.children.as_deref().unwrap_or_default();
                    match (&c.url, children.is_empty()) {
                        (Some(url), true) => check_url(url)?,
                        (None, false) => {}
                        _ => return Err("a custom item needs either a link or sub-items".into()),
                    }
                    if children.len() > MAX_ITEMS {
                        return Err("too many sub-items".into());
                    }
                    for child in children {
                        check_text(&child.label, "label")?;
                        check_url(&child.url)?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<Menus, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn test_parse_and_validate() {
        let menus = parse(
            r#"{
              "simple": [{"type": "boards"}, {"type": "wiki", "label": {"ko": "위키", "en": "Wiki"}}],
              "default": [
                {"label": "Help", "url": "https://example.org/wiki/Help"},
                {"label": "More", "children": [{"label": "Rules", "url": "/free/1"}]}
              ]
            }"#,
        )
        .unwrap();
        assert!(validate(&menus).is_ok());
        assert!(matches!(
            menus["simple"][0],
            MenuItem::Builtin(BuiltinItem {
                kind: BuiltinType::Boards,
                ..
            })
        ));
        // Round trip keeps the shape the client expects.
        let json = serde_json::to_value(&menus).unwrap();
        assert_eq!(json["simple"][0], serde_json::json!({"type": "boards"}));
        assert_eq!(json["default"][1]["children"][0]["url"], "/free/1");
    }

    #[test]
    fn test_rejects_bad_menus() {
        assert!(parse(r#"{"simple": [{"type": "nope"}]}"#).is_err());
        assert!(parse(r#"{"simple": [{"type": "boards", "extra": 1}]}"#).is_err());
        let bad = [
            r#"{"simple": [{"label": "x", "url": "javascript:alert(1)"}]}"#,
            r#"{"simple": [{"label": "x", "url": "//evil.example"}]}"#,
            r#"{"simple": [{"label": "x", "url": "/\\evil.example"}]}"#,
            r#"{"simple": [{"label": "", "url": "/a"}]}"#,
            r#"{"simple": [{"label": "x"}]}"#,
            r#"{"simple": [{"label": "x", "url": "/a", "children": [{"label": "y", "url": "/b"}]}]}"#,
            r#"{"Simple": [{"type": "boards"}]}"#,
            r#"{"simple": [{"type": "boards", "icon": "bad icon"}]}"#,
        ];
        for json in bad {
            let menus = parse(json).unwrap();
            assert!(validate(&menus).is_err(), "{} should be rejected", json);
        }
    }
}
