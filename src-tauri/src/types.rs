use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    pub title: String,
    pub source: String,
    pub cwd: Option<String>,
    pub model_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub pinned: bool,
    pub access_mode: String,
    pub source_path: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeDelivery {
    pub session_id: String,
    pub cwd: String,
    pub file_path: String,
    pub backup_path: Option<String>,
    pub written_hash: String,
    pub listed_in_client: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RewindPreview {
    pub changed_files: usize,
    pub commands_left_effects: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RewindOutcome {
    pub previous_leaf: Option<String>,
    pub restored_files: usize,
    pub not_restored: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub content: String,
    pub parts: Value,
    pub thinking: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub status: String,
    pub error: Option<String>,
    pub note: Option<String>,
    pub attachments: Value,
    pub imported: bool,
    pub compare_group: Option<String>,
    pub created_at: i64,
    pub sibling_ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPage {
    pub messages: Vec<Message>,
    pub has_more: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub kind: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub chat_id: String,
    pub message_id: Option<String>,
    pub title: String,
    pub source: String,
    pub snippet: Option<String>,
    pub updated_at: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    pub requires_key: bool,
    pub is_local: bool,
    pub is_custom: bool,
    pub has_key: bool,
    pub connected: bool,
    pub model_count: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub max_output: Option<i64>,
    pub max_input: Option<i64>,
    pub effort_levels: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyResult {
    pub provider_id: String,
    pub provider_name: String,
    pub model_count: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub today_cost: f64,
    pub month_cost: f64,
    pub month_input_tokens: i64,
    pub month_output_tokens: i64,
    pub budget: Option<f64>,
    pub by_provider: Vec<ProviderUsage>,
    pub unpriced_messages: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsage {
    pub provider: String,
    pub cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogEntry {
    pub id: i64,
    pub created_at: i64,
    pub host: String,
    pub provider: String,
    pub model: String,
    pub bytes_sent: i64,
    pub bytes_received: i64,
    pub status: String,
    pub message_count: i64,
    pub files: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PriceEntry {
    pub model: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportSource {
    pub source: String,
    pub path: String,
    pub exists: bool,
    pub file_count: usize,
    pub total_bytes: u64,
    pub imported_chats: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub source: String,
    pub files_done: usize,
    pub files_total: usize,
    pub chats_imported: usize,
    pub finished: bool,
    pub failed_files: usize,
    pub background: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    pub chat: ChatSummary,
    pub user_message: Option<Message>,
    pub assistant_messages: Vec<Message>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StreamPayload {
    pub message_id: String,
    pub kind: String,
    pub text: String,
    pub message: Option<Message>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SendRequest {
    pub chat_id: Option<String>,
    pub parent_id: Option<String>,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub model_refs: Vec<String>,
    pub access_mode: Option<String>,
    pub folder: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CustomProviderRequest {
    pub name: String,
    pub base_url: String,
    pub protocol: String,
    pub key: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub source: String,
    pub path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub name: String,
    pub source: String,
    pub target: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub text: String,
    pub status: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub chat_id: String,
    pub message_id: String,
    pub title: String,
    pub kind: String,
    pub path: String,
    pub size: i64,
    pub created_at: i64,
}
