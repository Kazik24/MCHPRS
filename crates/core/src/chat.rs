use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

static URL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("([a-zA-Z0-9§\\-:/]+\\.[a-zA-Z/0-9§\\-:_#]+(\\.[a-zA-Z/0-9.§\\-:#\\?\\+=_]+)?)")
        .unwrap()
});

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ColorCode {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
    Obfuscated,
    Bold,
    Strikethrough,
    Underline,
    Italic,
    Reset,
}

impl ColorCode {
    fn parse(code: char) -> Option<ColorCode> {
        Some(match code {
            '0' => ColorCode::Black,
            '1' => ColorCode::DarkBlue,
            '2' => ColorCode::DarkGreen,
            '3' => ColorCode::DarkAqua,
            '4' => ColorCode::DarkRed,
            '5' => ColorCode::DarkPurple,
            '6' => ColorCode::Gold,
            '7' => ColorCode::Gray,
            '8' => ColorCode::DarkGray,
            '9' => ColorCode::Blue,
            'a' => ColorCode::Green,
            'b' => ColorCode::Aqua,
            'c' => ColorCode::Red,
            'd' => ColorCode::LightPurple,
            'e' => ColorCode::Yellow,
            'f' => ColorCode::White,
            'k' => ColorCode::Obfuscated,
            'l' => ColorCode::Bold,
            'm' => ColorCode::Strikethrough,
            'n' => ColorCode::Underline,
            'o' => ColorCode::Italic,
            'r' => ColorCode::Reset,
            _ => return None,
        })
    }

    fn is_formatting(self) -> bool {
        use ColorCode::*;
        matches!(
            self,
            Obfuscated | Bold | Strikethrough | Underline | Italic | Reset
        )
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ChatColor {
    Hex(String),
    ColorCode(ColorCode),
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
enum ClickEventType {
    OpenUrl,
}

#[derive(Serialize, Debug, Clone)]
pub struct ClickEvent {
    action: ClickEventType,
    value: String,
}

/// This is only used for `ChatComponent` serialize
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(field: &bool) -> bool {
    !*field
}

pub struct ChatComponentBuilder {
    component: ChatComponent,
}

impl ChatComponentBuilder {
    pub fn new(text: String) -> Self {
        let component = ChatComponent {
            text,
            ..Default::default()
        };
        Self { component }
    }

    pub fn color_code(mut self, color: ColorCode) -> Self {
        self.component.color = Some(ChatColor::ColorCode(color));
        self
    }

    pub fn strikethrough(mut self, val: bool) -> Self {
        self.component.strikethrough = val;
        self
    }

    pub fn finish(self) -> ChatComponent {
        self.component
    }
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct ChatComponent {
    pub text: String,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underlined: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strikethrough: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub obfuscated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ChatColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clickEvent")]
    pub click_event: Option<ClickEvent>,
}

impl ChatComponent {
    pub fn ranked_message(prefix: &str, username: &str, message: &str) -> Vec<Self> {
        // LuckPerms prefixes use &#RRGGBB; the renderer accepts #RRGGBB.
        let prefix = prefix.replace("&#", "#");
        let mut components = Self::from_legacy_text(&format!("{prefix}{username} &8» "));
        // Player-authored text is literal, so it cannot inject rank styling.
        components.push(Self {
            text: message.to_owned(),
            color: Some(ChatColor::ColorCode(ColorCode::Gray)),
            ..Default::default()
        });
        components
    }

    pub fn player_joined(username: &str) -> Vec<Self> {
        Self::from_legacy_text(&format!("&8&l[&2&l+&8&l]&7 {username}"))
    }

    pub fn player_left(username: &str) -> Vec<Self> {
        Self::from_legacy_text(&format!("&8&l[&4&l-&8&l]&7 {username}"))
    }

    pub fn from_legacy_text(message: &str) -> Vec<ChatComponent> {
        let mut components = Vec::new();

        let mut cur_component: ChatComponent = Default::default();

        let mut chars = message.chars();
        'main_loop: while let Some(c) = chars.next() {
            if c == '&' {
                if let Some(code) = chars.next() {
                    if let Some(color) = ColorCode::parse(code) {
                        let make_new = !cur_component.text.is_empty();
                        if color.is_formatting() && make_new {
                            components.push(cur_component.clone());
                            cur_component.text.clear();
                        }
                        match color {
                            ColorCode::Bold => cur_component.bold = true,
                            ColorCode::Italic => cur_component.italic = true,
                            ColorCode::Underline => cur_component.underlined = true,
                            ColorCode::Strikethrough => cur_component.strikethrough = true,
                            ColorCode::Obfuscated => cur_component.obfuscated = true,
                            _ => {
                                components.push(cur_component);
                                cur_component = Default::default();
                                cur_component.color = Some(ChatColor::ColorCode(color));
                            }
                        }
                        continue;
                    }
                    cur_component.text.push(c);
                    cur_component.text.push(code);
                    continue;
                }
            }
            if c == '#' {
                let mut hex = String::from(c);
                for _ in 0..6 {
                    if let Some(c) = chars.next() {
                        hex.push(c);
                        if !c.is_ascii_hexdigit() {
                            cur_component.text += &hex;
                            continue 'main_loop;
                        }
                    } else {
                        cur_component.text += &hex;
                        continue 'main_loop;
                    }
                }
                components.push(cur_component);
                cur_component = Default::default();
                cur_component.color = Some(ChatColor::Hex(hex));
                continue;
            }
            cur_component.text.push(c);
        }
        components.push(cur_component);

        // Split URLs while cloning only the formatting, rather than the entire text.
        let mut linked_components = Vec::with_capacity(components.len());
        for mut component in components {
            let mut last = 0;
            let text = std::mem::take(&mut component.text);

            for match_ in URL_REGEX.find_iter(&text) {
                let index = match_.start();
                let matched = match_.as_str();
                if last != index {
                    linked_components.push(Self {
                        text: text[last..index].to_owned(),
                        ..component.clone()
                    });
                }
                linked_components.push(Self {
                    text: matched.to_owned(),
                    click_event: Some(ClickEvent {
                        action: ClickEventType::OpenUrl,
                        value: matched.to_owned(),
                    }),
                    ..component.clone()
                });
                last = index + matched.len();
            }
            if last < text.len() {
                component.text = text[last..].to_owned();
                linked_components.push(component);
            }
        }

        linked_components
    }

    pub fn encode_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_ascii_digits_remain_literal_in_legacy_hex_colors() {
        let text = "#١23abc";
        let components = ChatComponent::from_legacy_text(text);
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].text, text);
        assert!(components[0].color.is_none());

        let components = ChatComponent::from_legacy_text("#A1b2C3text");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].text, "text");
        assert!(matches!(&components[0].color, Some(ChatColor::Hex(color)) if color == "#A1b2C3"));
    }

    #[test]
    fn url_splits_preserve_text_and_formatting() {
        let components = ChatComponent::from_legacy_text("&lbefore example.com after");
        assert_eq!(components.len(), 3);
        assert!(components.iter().all(|component| component.bold));
        assert_eq!(components[0].text, "before ");
        assert_eq!(components[1].text, "example.com");
        assert_eq!(components[2].text, " after");
        assert!(components[0].click_event.is_none());
        assert_eq!(
            components[1].click_event.as_ref().unwrap().value,
            "example.com"
        );
        assert!(components[2].click_event.is_none());
    }
}
