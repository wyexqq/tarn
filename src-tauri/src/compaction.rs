use serde::Serialize;

use crate::chat::context_text;
use crate::types::Message;

pub const CHARS_PER_TOKEN_ESTIMATE: usize = 3;
pub const DEFAULT_AUTO_COMPACT_TOKENS: i64 = 400_000;
pub const AUTO_COMPACT_SETTING: &str = "autoCompactTokens";
const SHARE_KEPT_AS_RECENT: usize = 4;
const MODEL_WINDOW_PERCENT_BEFORE_COMPACT: i64 = 85;

pub const COMPACT_MODEL_SETTING: &str = "compactModel";
pub const PIECE_TOKENS: i64 = 70_000;
const MODEL_WINDOW_PERCENT_FOR_PIECE: i64 = 60;

pub const SUMMARY_INSTRUCTION: &str = "Сожми разговор ниже в подробное содержание, по которому его можно продолжить без исходного текста. Сохрани: что просил пользователь, его предпочтения и замечания дословно, принятые решения, изменённые файлы и команды, ошибки и как они решены, что сделано и что осталось. Если в начале дано содержание более ранней части, перенеси его в новое содержание, ничего важного не теряя. Пиши на языке разговора. Ответь только содержанием.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compaction {
    pub up_to_message_id: String,
    pub summary: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CompactionPlan {
    pub up_to_message_id: String,
    pub pieces: Vec<String>,
    pub tokens_before: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPart {
    pub kind: String,
    pub title: String,
    pub path: Option<String>,
    pub tokens: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub parts: Vec<ContextPart>,
    pub used_tokens: i64,
    pub compact_at_tokens: Option<i64>,
    pub model_limit_tokens: Option<i64>,
    pub is_compacted: bool,
    pub compact_model_ref: String,
}

pub fn tokens_of_chars(chars: usize) -> i64 {
    (chars / CHARS_PER_TOKEN_ESTIMATE) as i64
}

pub fn tokens_of_text(text: &str) -> i64 {
    tokens_of_chars(text.chars().count())
}

pub fn compact_threshold(setting: Option<&str>, model_limit: Option<i64>) -> Option<i64> {
    let wanted = setting.and_then(|value| value.trim().parse::<i64>().ok()).unwrap_or(DEFAULT_AUTO_COMPACT_TOKENS);
    if wanted <= 0 {
        return None;
    }
    let safe_for_model = model_limit.map(|limit| limit * MODEL_WINDOW_PERCENT_BEFORE_COMPACT / 100);
    Some(safe_for_model.map_or(wanted, |safe| wanted.min(safe)))
}

pub fn piece_chars_for(model_limit: Option<i64>) -> usize {
    let share_of_window = model_limit.map(|limit| limit * MODEL_WINDOW_PERCENT_FOR_PIECE / 100);
    let tokens = share_of_window.map_or(PIECE_TOKENS, |share| share.min(PIECE_TOKENS)).max(1);
    tokens as usize * CHARS_PER_TOKEN_ESTIMATE
}

pub fn piece_request(summary_so_far: &str, piece: &str) -> String {
    if summary_so_far.trim().is_empty() {
        return piece.to_string();
    }
    format!("Содержание разговора до этого места:
{summary_so_far}

Дальше разговор шёл так:
{piece}")
}

pub fn messages_after_summary<'a>(path: &'a [Message], compaction: Option<&Compaction>) -> &'a [Message] {
    let position = compaction.and_then(|summary| path.iter().position(|message| message.id == summary.up_to_message_id));
    match position {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

pub fn summary_section(compaction: Option<&Compaction>) -> Option<String> {
    compaction.map(|summary| format!("Краткое содержание более ранней части этого разговора:\n{}", summary.summary))
}

fn counts_for_context(message: &Message) -> bool {
    message.status != "streaming" && (message.role == "user" || message.role == "assistant")
}

fn speaker_of(message: &Message) -> &'static str {
    if message.role == "user" {
        "Пользователь"
    } else {
        "Ассистент"
    }
}

fn split_long_entry(entry: &str, piece_chars: usize) -> Vec<String> {
    let characters: Vec<char> = entry.chars().collect();
    characters.chunks(piece_chars.max(1)).map(|chunk| chunk.iter().collect()).collect()
}

pub fn split_into_pieces(entries: Vec<String>, piece_chars: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;
    for entry in entries {
        let entry_chars = entry.chars().count();
        if current_chars + entry_chars > piece_chars && !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        if entry_chars > piece_chars {
            pieces.extend(split_long_entry(&entry, piece_chars));
            continue;
        }
        current.push_str(&entry);
        current_chars += entry_chars;
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

pub struct PlanLimits {
    pub threshold_tokens: i64,
    pub fixed_chars: usize,
    pub piece_chars: usize,
    pub forced: bool,
}

pub fn plan_compaction(recent_path: &[Message], previous: Option<&Compaction>, limits: &PlanLimits) -> Option<CompactionPlan> {
    let usable: Vec<(&Message, String)> = recent_path
        .iter()
        .filter(|message| counts_for_context(message))
        .map(|message| (message, context_text(message)))
        .collect();
    let previous_chars = previous.map_or(0, |summary| summary.summary.chars().count());
    let message_chars: usize = usable.iter().map(|(_, text)| text.chars().count()).sum();
    let total_chars = limits.fixed_chars + previous_chars + message_chars;
    let threshold_chars = limits.threshold_tokens.max(0) as usize * CHARS_PER_TOKEN_ESTIMATE;
    if !limits.forced && total_chars < threshold_chars {
        return None;
    }

    let recent_budget = (threshold_chars / SHARE_KEPT_AS_RECENT).min(message_chars / SHARE_KEPT_AS_RECENT);
    let mut first_kept = usable.len();
    let mut recent_chars = 0usize;
    while first_kept > 0 {
        let length = usable[first_kept - 1].1.chars().count();
        let last_message_is_always_kept = first_kept == usable.len() && usable[first_kept - 1].0.role == "user";
        if recent_chars + length > recent_budget && !last_message_is_always_kept {
            break;
        }
        recent_chars += length;
        first_kept -= 1;
    }
    while first_kept < usable.len() && usable[first_kept].0.role != "user" {
        first_kept += 1;
    }
    if first_kept == 0 {
        return None;
    }

    let mut entries = Vec::new();
    if let Some(summary) = previous {
        entries.push(format!("Содержание ещё более раннего разговора:
{}

", summary.summary));
    }
    for (message, text) in &usable[..first_kept] {
        entries.push(format!("{}:
{text}

", speaker_of(message)));
    }
    Some(CompactionPlan {
        up_to_message_id: usable[first_kept - 1].0.id.clone(),
        pieces: split_into_pieces(entries, limits.piece_chars),
        tokens_before: tokens_of_chars(total_chars),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(id: &str, role: &str, content: &str) -> Message {
        Message {
            id: id.into(),
            chat_id: "c".into(),
            parent_id: None,
            role: role.into(),
            content: content.into(),
            parts: json!([]),
            thinking: None,
            provider: None,
            model: None,
            input_tokens: None,
            output_tokens: None,
            cost: None,
            status: "done".into(),
            error: None,
            note: None,
            attachments: json!([]),
            imported: false,
            compare_group: None,
            created_at: 0,
            sibling_ids: vec![],
        }
    }

    fn long_chat() -> Vec<Message> {
        vec![
            message("u1", "user", &"а".repeat(300)),
            message("a1", "assistant", &"б".repeat(300)),
            message("u2", "user", &"в".repeat(300)),
            message("a2", "assistant", &"г".repeat(300)),
            message("u3", "user", "новый вопрос"),
        ]
    }

    fn limits(threshold_tokens: i64) -> PlanLimits {
        PlanLimits { threshold_tokens, fixed_chars: 0, piece_chars: 100_000, forced: false }
    }

    #[test]
    fn threshold_respects_the_setting_and_the_model_window() {
        assert_eq!(compact_threshold(None, None), Some(400_000));
        assert_eq!(compact_threshold(Some("100000"), None), Some(100_000));
        assert_eq!(compact_threshold(Some("100000"), Some(1_000_000)), Some(100_000));
        assert_eq!(compact_threshold(None, Some(200_000)), Some(170_000));
        assert_eq!(compact_threshold(Some("0"), Some(200_000)), None);
        assert_eq!(compact_threshold(Some("ерунда"), None), Some(400_000));
    }

    #[test]
    fn nothing_happens_below_the_threshold() {
        assert_eq!(plan_compaction(&long_chat(), None, &limits(1_000)), None);
    }

    #[test]
    fn fixed_parts_count_towards_the_threshold() {
        let with_big_memory = PlanLimits { fixed_chars: 3_000, ..limits(1_000) };
        assert!(plan_compaction(&long_chat(), None, &with_big_memory).is_some());
    }

    #[test]
    fn old_messages_are_summarized_and_recent_turn_stays() {
        let plan = plan_compaction(&long_chat(), None, &limits(300)).unwrap();
        assert_eq!(plan.up_to_message_id, "a2");
        assert!(plan.pieces[0].starts_with("Пользователь:\nааа"));
        assert!(plan.pieces[0].contains("Ассистент:\nггг"));
        assert!(!plan.pieces[0].contains("новый вопрос"));
        assert_eq!(plan.tokens_before, 404);
    }

    #[test]
    fn what_is_kept_always_starts_with_a_user_message() {
        let path = vec![
            message("u1", "user", &"а".repeat(1_000)),
            message("a1", "assistant", &"б".repeat(10)),
            message("a2", "assistant", &"г".repeat(10)),
            message("u2", "user", "х"),
        ];
        let plan = plan_compaction(&path, None, &limits(100)).unwrap();
        assert_eq!(plan.up_to_message_id, "a2");
    }

    #[test]
    fn a_single_huge_question_cannot_be_compacted() {
        let path = vec![message("u1", "user", &"а".repeat(10_000))];
        assert_eq!(plan_compaction(&path, None, &limits(100)), None);
    }

    #[test]
    fn previous_summary_goes_into_the_new_one() {
        let previous = Compaction { up_to_message_id: "старый".into(), summary: "старое содержание".into() };
        let plan = plan_compaction(&long_chat(), Some(&previous), &limits(300)).unwrap();
        assert!(plan.pieces[0].starts_with("Содержание ещё более раннего разговора:\nстарое содержание"));
    }

    #[test]
    fn long_history_is_split_into_pieces_without_losing_anything() {
        let small = PlanLimits { piece_chars: 700, ..limits(300) };
        let plan = compaction_plan_or_panic(&long_chat(), &small);
        assert_eq!(plan.pieces.len(), 2);
        assert!(plan.pieces[0].contains('а') && plan.pieces[0].contains('б'));
        assert!(plan.pieces[1].contains('в') && plan.pieces[1].contains('г'));
    }

    #[test]
    fn a_message_longer_than_a_piece_is_cut_into_several() {
        let pieces = split_into_pieces(vec!["ab".repeat(5), "c".to_string()], 4);
        assert_eq!(pieces, ["abab", "abab", "ab", "c"]);
    }

    #[test]
    fn piece_size_follows_the_smaller_of_limit_and_model_window() {
        assert_eq!(piece_chars_for(None), PIECE_TOKENS as usize * CHARS_PER_TOKEN_ESTIMATE);
        assert_eq!(piece_chars_for(Some(1_000_000)), PIECE_TOKENS as usize * CHARS_PER_TOKEN_ESTIMATE);
        assert_eq!(piece_chars_for(Some(10_000)), 6_000 * CHARS_PER_TOKEN_ESTIMATE);
    }

    #[test]
    fn next_piece_carries_the_summary_so_far() {
        assert_eq!(piece_request("", "кусок"), "кусок");
        let request = piece_request("сводка", "кусок");
        assert!(request.starts_with("Содержание разговора до этого места:
сводка"));
        assert!(request.ends_with("кусок"));
    }

    fn compaction_plan_or_panic(path: &[Message], limits: &PlanLimits) -> CompactionPlan {
        plan_compaction(path, None, limits).expect("план сжатия")
    }

    #[test]
    fn manual_compaction_works_below_the_threshold() {
        let mut path = long_chat();
        path.pop();
        let forced = PlanLimits { forced: true, ..limits(1_000_000) };
        let plan = plan_compaction(&path, None, &forced).unwrap();
        assert_eq!(plan.up_to_message_id, "a2");
        assert_eq!(plan_compaction(&path[..1], None, &forced), None);
    }

    #[test]
    fn a_rewound_chat_ignores_a_summary_of_messages_it_no_longer_has() {
        let path = long_chat();
        let summary = Compaction { up_to_message_id: "a1".into(), summary: "с".into() };
        assert_eq!(messages_after_summary(&path, Some(&summary)).len(), 3);
        let elsewhere = Compaction { up_to_message_id: "другая ветка".into(), summary: "с".into() };
        assert_eq!(messages_after_summary(&path, Some(&elsewhere)).len(), 5);
    }
}
