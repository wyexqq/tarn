use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::types::Attachment;

const DEFAULT_IMAGE_NAME: &str = "изображение";
const DATA_URL_BASE64_MARKER: &str = ";base64,";

#[derive(Debug, Clone, PartialEq)]
pub struct StoredImage {
    pub id: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl StoredImage {
    pub fn from_base64(mime: &str, encoded: &str) -> Option<StoredImage> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
        if bytes.is_empty() {
            return None;
        }
        let id = format!("{:x}", Sha256::digest(&bytes));
        Some(StoredImage { id, mime: mime.to_string(), bytes })
    }

    pub fn from_data_url(url: &str) -> Option<StoredImage> {
        let without_scheme = url.strip_prefix("data:")?;
        let (mime, encoded) = without_scheme.split_once(DATA_URL_BASE64_MARKER)?;
        StoredImage::from_base64(mime, encoded)
    }

    pub fn from_anthropic_block(block: &Value) -> Option<StoredImage> {
        if block["type"] != "image" || block["source"]["type"] != "base64" {
            return None;
        }
        let mime = block["source"]["media_type"].as_str()?;
        let encoded = block["source"]["data"].as_str()?;
        StoredImage::from_base64(mime, encoded)
    }

    pub fn attachment(&self, name: &str) -> Attachment {
        Attachment {
            name: name.to_string(),
            mime: self.mime.clone(),
            kind: "image".to_string(),
            size: self.bytes.len() as u64,
            text: None,
            data_base64: None,
            image_id: Some(self.id.clone()),
        }
    }

    pub fn default_attachment(&self) -> Attachment {
        self.attachment(DEFAULT_IMAGE_NAME)
    }
}

pub fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_picture_gets_same_id_and_bad_data_is_rejected() {
        let first = StoredImage::from_base64("image/png", "aGVsbG8=").unwrap();
        let second = StoredImage::from_data_url("data:image/png;base64,aGVsbG8=").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.bytes, b"hello");
        assert!(StoredImage::from_base64("image/png", "не base64").is_none());
        assert!(StoredImage::from_data_url("https://example.com/a.png").is_none());
    }

    #[test]
    fn reads_only_inline_anthropic_images() {
        let inline = json!({ "type": "image", "source": { "type": "base64", "media_type": "image/webp", "data": "aGVsbG8=" } });
        assert_eq!(StoredImage::from_anthropic_block(&inline).unwrap().mime, "image/webp");
        let by_link = json!({ "type": "image", "source": { "type": "url", "url": "https://example.com/a.png" } });
        assert!(StoredImage::from_anthropic_block(&by_link).is_none());
    }
}
