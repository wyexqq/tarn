use serde::Serialize;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextRun {
    pub text: String,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub spoiler: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub quote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    spoiler: bool,
    code: bool,
    quote: bool,
    link: Option<String>,
}

#[derive(Default)]
struct Runs {
    list: Vec<TextRun>,
}

impl Runs {
    fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.list.last_mut() {
            if style_of(last) == *style {
                last.text.push_str(text);
                return;
            }
        }
        self.list.push(TextRun {
            text: text.to_string(),
            bold: style.bold,
            italic: style.italic,
            underline: style.underline,
            strike: style.strike,
            spoiler: style.spoiler,
            code: style.code,
            quote: style.quote,
            link: style.link.clone(),
        });
    }
}

fn style_of(run: &TextRun) -> Style {
    Style {
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        strike: run.strike,
        spoiler: run.spoiler,
        code: run.code,
        quote: run.quote,
        link: run.link.clone(),
    }
}

pub fn plain(text: &str) -> Vec<TextRun> {
    let mut runs = Runs::default();
    runs.push(text, &Style::default());
    runs.list
}

pub fn parse(text: &str, mode: Option<&str>) -> Result<Vec<TextRun>, String> {
    match mode.map(str::to_lowercase).as_deref() {
        Some("html") => parse_html(text),
        Some("markdownv2") => parse_markdown_v2(text),
        Some("markdown") => parse_markdown_legacy(text),
        Some("") | None => Ok(plain(text)),
        Some(other) => Err(format!("Unsupported parse_mode \"{other}\"")),
    }
}

const HTML_TAGS: [&str; 15] = [
    "b", "strong", "i", "em", "u", "ins", "s", "strike", "del", "a", "code", "pre", "span", "tg-spoiler", "blockquote",
];

fn attribute_value(attributes: &str, name: &str) -> Option<String> {
    let lowered = attributes.to_ascii_lowercase();
    let start = lowered.find(&format!("{name}="))? + name.len() + 1;
    let rest = &attributes[start..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let inner = &rest[1..];
        inner.find(quote).map(|end| decode_html_entities(&inner[..end]))
    } else {
        Some(rest.split_whitespace().next().unwrap_or_default().to_string())
    }
}

fn apply_tag(style: &mut Style, tag: &str, attributes: &str, offset: usize) -> Result<(), String> {
    match tag {
        "b" | "strong" => style.bold = true,
        "i" | "em" => style.italic = true,
        "u" | "ins" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "tg-spoiler" => style.spoiler = true,
        "code" | "pre" => style.code = true,
        "blockquote" => style.quote = true,
        "a" => style.link = Some(attribute_value(attributes, "href").unwrap_or_default()),
        "span" => {
            if attribute_value(attributes, "class").as_deref() != Some("tg-spoiler") {
                return Err(format!("Tag \"span\" must have class \"tg-spoiler\" at byte offset {offset}"));
            }
            style.spoiler = true;
        }
        _ => {}
    }
    Ok(())
}

fn entity_text(entity: &str) -> Option<String> {
    let named = match entity {
        "lt" => Some('<'),
        "gt" => Some('>'),
        "amp" => Some('&'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => None,
    };
    if let Some(letter) = named {
        return Some(letter.to_string());
    }
    let number = entity.strip_prefix('#')?;
    let code = match number.strip_prefix('x').or_else(|| number.strip_prefix('X')) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => number.parse().ok()?,
    };
    char::from_u32(code).map(String::from)
}

fn decode_html_entities(text: &str) -> String {
    let mut decoded = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find(';').and_then(|end| entity_text(&after[..end]).map(|value| (end, value))) {
            Some((end, value)) => {
                decoded.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = after;
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn parse_html(text: &str) -> Result<Vec<TextRun>, String> {
    let mut runs = Runs::default();
    let mut open: Vec<(String, Style)> = Vec::new();
    let mut position = 0;
    while position < text.len() {
        let current = open.last().map(|(_, style)| style.clone()).unwrap_or_default();
        let rest = &text[position..];
        let Some(tag_start) = rest.find('<') else {
            runs.push(&decode_html_entities(rest), &current);
            break;
        };
        runs.push(&decode_html_entities(&rest[..tag_start]), &current);
        let offset = position + tag_start;
        let after_bracket = &text[offset + 1..];
        let Some(tag_end) = after_bracket.find('>') else {
            return Err(format!("Unclosed start tag at byte offset {offset}"));
        };
        let inside = after_bracket[..tag_end].trim();
        position = offset + 1 + tag_end + 1;
        if let Some(closing) = inside.strip_prefix('/') {
            let name = closing.trim().to_lowercase();
            match open.pop() {
                Some((expected, _)) if expected == name => {}
                Some((expected, _)) => {
                    return Err(format!("Unmatched end tag at byte offset {offset}, expected \"</{expected}>\", found \"</{name}>\""));
                }
                None => return Err(format!("Unexpected end tag at byte offset {offset}")),
            }
            continue;
        }
        let name_end = inside.find(char::is_whitespace).unwrap_or(inside.len());
        let name = inside[..name_end].trim_end_matches('/').to_lowercase();
        if !HTML_TAGS.contains(&name.as_str()) {
            return Err(format!("Unsupported start tag \"{name}\" at byte offset {offset}"));
        }
        let mut style = current;
        apply_tag(&mut style, &name, &inside[name_end..], offset)?;
        open.push((name, style));
    }
    if let Some((name, _)) = open.last() {
        return Err(format!("Can't find end tag corresponding to start tag \"{name}\""));
    }
    Ok(runs.list)
}

const MARKDOWN_V2_RESERVED: [char; 18] = ['_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!'];

fn entity_name(marker: &str) -> &'static str {
    match marker {
        "*" => "Bold",
        "_" => "Italic",
        "__" => "Underline",
        "~" => "Strikethrough",
        "||" => "Spoiler",
        _ => "TextUrl",
    }
}

fn toggle(style: &mut Style, marker: &str) {
    match marker {
        "*" => style.bold = !style.bold,
        "_" => style.italic = !style.italic,
        "__" => style.underline = !style.underline,
        "~" => style.strike = !style.strike,
        "||" => style.spoiler = !style.spoiler,
        _ => {}
    }
}

fn read_link_target(text: &str, from: usize) -> Option<(String, usize)> {
    let rest = text.get(from..)?;
    let inner = rest.strip_prefix('(')?;
    let mut target = String::new();
    let mut letters = inner.char_indices();
    while let Some((index, letter)) = letters.next() {
        match letter {
            '\\' => {
                if let Some((_, escaped)) = letters.next() {
                    target.push(escaped);
                }
            }
            ')' => return Some((target, from + 1 + index + 1)),
            other => target.push(other),
        }
    }
    None
}

struct MarkdownState {
    runs: Runs,
    style: Style,
    open_markers: Vec<(String, usize)>,
    link_start: Option<(usize, usize)>,
}

impl MarkdownState {
    fn new() -> Self {
        MarkdownState { runs: Runs::default(), style: Style::default(), open_markers: Vec::new(), link_start: None }
    }

    fn switch(&mut self, marker: &str, offset: usize) {
        let is_open = self.open_markers.iter().any(|(open, _)| open == marker);
        if is_open {
            self.open_markers.retain(|(open, _)| open != marker);
        } else {
            self.open_markers.push((marker.to_string(), offset));
        }
        toggle(&mut self.style, marker);
    }

    fn finish(self) -> Result<Vec<TextRun>, String> {
        if let Some((marker, offset)) = self.open_markers.first() {
            return Err(format!("Can't find end of {} entity at byte offset {offset}", entity_name(marker)));
        }
        if let Some((_, offset)) = self.link_start {
            return Err(format!("Can't find end of TextUrl entity at byte offset {offset}"));
        }
        Ok(self.runs.list)
    }

    fn finish_legacy(self) -> Result<Vec<TextRun>, String> {
        if let Some((_, offset)) = self.open_markers.first() {
            return Err(format!("Can't find end of the entity starting at byte offset {offset}"));
        }
        Ok(self.runs.list)
    }
}

fn without_language_line(block: String) -> String {
    match block.split_once('\n') {
        Some((first_line, code)) if !first_line.contains(' ') => code.to_string(),
        _ => block,
    }
}

fn read_code(text: &str, from: usize, fence: &str) -> Option<(String, usize)> {
    let body_start = from + fence.len();
    let rest = text.get(body_start..)?;
    let mut body = String::new();
    let mut letters = rest.char_indices();
    while let Some((index, letter)) = letters.next() {
        if letter == '\\' {
            if let Some((_, escaped)) = letters.next() {
                body.push(escaped);
            }
            continue;
        }
        if rest[index..].starts_with(fence) {
            let code = if fence == "```" { without_language_line(body) } else { body };
            return Some((code, body_start + index + fence.len()));
        }
        body.push(letter);
    }
    None
}

fn parse_markdown_v2(text: &str) -> Result<Vec<TextRun>, String> {
    let mut state = MarkdownState::new();
    let mut position = 0;
    while let Some(letter) = text[position..].chars().next() {
        let offset = position;
        let rest = &text[position..];
        position += letter.len_utf8();
        match letter {
            '\\' => {
                if let Some(escaped) = text[position..].chars().next() {
                    state.runs.push(&escaped.to_string(), &state.style);
                    position += escaped.len_utf8();
                }
            }
            '`' => {
                let fence = if rest.starts_with("```") { "```" } else { "`" };
                let (code, end) = read_code(text, offset, fence).ok_or_else(|| format!("Can't find end of Code entity at byte offset {offset}"))?;
                let mut code_style = state.style.clone();
                code_style.code = true;
                state.runs.push(&code, &code_style);
                position = end;
            }
            '_' if rest.starts_with("__") => {
                state.switch("__", offset);
                position += 1;
            }
            '|' if rest.starts_with("||") => {
                state.switch("||", offset);
                position += 1;
            }
            '*' | '_' | '~' => state.switch(&letter.to_string(), offset),
            '[' => {
                if state.link_start.is_some() {
                    return Err("Character '[' is reserved and must be escaped with the preceding '\\'".to_string());
                }
                state.link_start = Some((state.runs.list.len(), offset));
                state.runs.list.push(TextRun::default());
            }
            ']' => {
                let Some((first_run, _)) = state.link_start.take() else {
                    return Err("Character ']' is reserved and must be escaped with the preceding '\\'".to_string());
                };
                let (target, end) = read_link_target(text, position).ok_or_else(|| format!("Can't find end of TextUrl entity at byte offset {offset}"))?;
                for run in state.runs.list.iter_mut().skip(first_run) {
                    run.link = Some(target.clone());
                }
                state.runs.list.retain(|run| !run.text.is_empty());
                position = end;
            }
            '>' if offset == 0 || text[..offset].ends_with('\n') => {
                state.style.quote = true;
            }
            '\n' => {
                state.style.quote = false;
                state.runs.push("\n", &state.style);
            }
            other if MARKDOWN_V2_RESERVED.contains(&other) => {
                return Err(format!("Character '{other}' is reserved and must be escaped with the preceding '\\'"));
            }
            other => {
                let mut buffer = [0u8; 4];
                let piece: &str = other.encode_utf8(&mut buffer);
                state.runs.push(piece, &state.style);
            }
        }
    }
    state.finish()
}

fn parse_markdown_legacy(text: &str) -> Result<Vec<TextRun>, String> {
    let mut state = MarkdownState::new();
    let mut position = 0;
    while let Some(letter) = text[position..].chars().next() {
        let offset = position;
        let rest = &text[position..];
        position += letter.len_utf8();
        match letter {
            '\\' if text[position..].starts_with(['_', '*', '`', '[']) => {
                let escaped = &text[position..position + 1];
                state.runs.push(escaped, &state.style);
                position += 1;
            }
            '`' => {
                let fence = if rest.starts_with("```") { "```" } else { "`" };
                let (code, end) = read_code(text, offset, fence).ok_or_else(|| format!("Can't find end of the entity starting at byte offset {offset}"))?;
                let mut code_style = state.style.clone();
                code_style.code = true;
                state.runs.push(&code, &code_style);
                position = end;
            }
            '*' | '_' => state.switch(&letter.to_string(), offset),
            '[' => {
                let Some(close) = text[position..].find(']') else {
                    return Err(format!("Can't find end of the entity starting at byte offset {offset}"));
                };
                let label = &text[position..position + close];
                let (target, end) = read_link_target(text, position + close + 1).ok_or_else(|| format!("Can't find end of the entity starting at byte offset {offset}"))?;
                let mut link_style = state.style.clone();
                link_style.link = Some(target);
                state.runs.push(label, &link_style);
                position = end;
            }
            other => {
                let mut buffer = [0u8; 4];
                let piece: &str = other.encode_utf8(&mut buffer);
                state.runs.push(piece, &state.style);
            }
        }
    }
    state.finish_legacy()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(runs: &[TextRun]) -> Vec<&str> {
        runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn plain_text_stays_one_run() {
        let runs = parse("Привет <b>", None).unwrap();
        assert_eq!(texts(&runs), ["Привет <b>"]);
    }

    #[test]
    fn html_tags_become_styles() {
        let runs = parse("Привет, <b>Анна</b> &amp; <a href=\"https://t.me\">ссылка</a>", Some("HTML")).unwrap();
        assert_eq!(texts(&runs), ["Привет, ", "Анна", " & ", "ссылка"]);
        assert!(runs[1].bold);
        assert_eq!(runs[3].link.as_deref(), Some("https://t.me"));
    }

    #[test]
    fn html_errors_look_like_telegram() {
        assert_eq!(parse("a<br>b", Some("HTML")).unwrap_err(), "Unsupported start tag \"br\" at byte offset 1");
        assert!(parse("<b>жирный", Some("HTML")).unwrap_err().contains("start tag \"b\""));
        assert!(parse("<b><i>x</b></i>", Some("HTML")).unwrap_err().contains("expected \"</i>\""));
        assert!(parse("5 < 6", Some("HTML")).is_err());
    }

    #[test]
    fn markdown_v2_wants_reserved_characters_escaped() {
        assert_eq!(
            parse("Цена 100.", Some("MarkdownV2")).unwrap_err(),
            "Character '.' is reserved and must be escaped with the preceding '\\'"
        );
        let runs = parse("*Цена* 100\\.", Some("MarkdownV2")).unwrap();
        assert_eq!(texts(&runs), ["Цена", " 100."]);
        assert!(runs[0].bold);
    }

    #[test]
    fn markdown_v2_reads_links_code_and_underline() {
        let runs = parse("[сайт](https://example.com) `x_y` __под__", Some("MarkdownV2")).unwrap();
        assert_eq!(runs[0].link.as_deref(), Some("https://example.com"));
        assert!(runs.iter().any(|run| run.code && run.text == "x_y"));
        assert!(runs.iter().any(|run| run.underline && run.text == "под"));
    }

    #[test]
    fn markdown_v2_reports_unclosed_entities() {
        assert!(parse("*жирный", Some("MarkdownV2")).unwrap_err().contains("Bold"));
    }

    #[test]
    fn legacy_markdown_is_lenient_about_punctuation() {
        let runs = parse("*Итого:* 100.", Some("Markdown")).unwrap();
        assert_eq!(texts(&runs), ["Итого:", " 100."]);
        assert!(parse("snake_case", Some("Markdown")).is_err());
    }
}
