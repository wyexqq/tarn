use crate::types::{ChatSummary, Message};

fn role_heading(message: &Message) -> String {
    match message.role.as_str() {
        "user" => "Ты".to_string(),
        _ => message.model.clone().unwrap_or_else(|| "Ассистент".to_string()),
    }
}

pub fn to_markdown(chat: &ChatSummary, messages: &[Message]) -> String {
    let mut output = format!("# {}\n\n", chat.title);
    for message in messages {
        output.push_str(&format!("## {}\n\n", role_heading(message)));
        output.push_str(&message.content);
        output.push_str("\n\n");
        if let Some(parts) = message.parts.as_array() {
            for part in parts.iter().filter(|part| part["type"] == "tool_call") {
                let name = part["name"].as_str().unwrap_or("инструмент");
                let input = part["input"].as_str().unwrap_or_default();
                output.push_str(&format!("> Действие {name}: `{}`\n\n", input.replace('`', "'")));
            }
        }
    }
    output
}

pub fn to_json(chat: &ChatSummary, messages: &[Message]) -> Result<String, String> {
    let document = serde_json::json!({ "chat": chat, "messages": messages });
    serde_json::to_string_pretty(&document).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_contains_title_roles_and_tools() {
        let chat = ChatSummary {
            id: "c".into(),
            title: "Заголовок".into(),
            source: "local".into(),
            cwd: None,
            model_ref: None,
            created_at: 0,
            updated_at: 0,
            pinned: false,
            access_mode: "chat".to_string(),
            source_path: None,
        };
        let message = Message {
            id: "1".into(),
            chat_id: "c".into(),
            parent_id: None,
            role: "assistant".into(),
            content: "Ответ".into(),
            parts: json!([{ "type": "tool_call", "name": "Bash", "input": "ls" }]),
            thinking: None,
            provider: None,
            model: Some("gpt".into()),
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
        };
        let text = to_markdown(&chat, &[message]);
        assert!(text.starts_with("# Заголовок"));
        assert!(text.contains("## gpt"));
        assert!(text.contains("Действие Bash: `ls`"));
    }
}
