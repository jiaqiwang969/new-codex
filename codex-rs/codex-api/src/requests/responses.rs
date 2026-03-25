use codex_protocol::models::ResponseItem;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    None,
    Zstd,
}

pub(crate) fn attach_item_ids(payload_json: &mut Value, original_items: &[ResponseItem]) {
    let Some(input_value) = payload_json.get_mut("input") else {
        return;
    };
    let Value::Array(items) = input_value else {
        return;
    };

    for (value, item) in items.iter_mut().zip(original_items.iter()) {
        if let ResponseItem::Reasoning { id, .. }
        | ResponseItem::Message { id: Some(id), .. }
        | ResponseItem::WebSearchCall { id: Some(id), .. }
        | ResponseItem::FunctionCall { id: Some(id), .. }
        | ResponseItem::ToolSearchCall { id: Some(id), .. }
        | ResponseItem::LocalShellCall { id: Some(id), .. }
        | ResponseItem::CustomToolCall { id: Some(id), .. } = item
        {
            if id.is_empty() {
                continue;
            }

            if let Some(obj) = value.as_object_mut() {
                obj.insert("id".to_string(), Value::String(id.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn attach_item_ids_inserts_non_empty_ids() {
        let mut payload_json = json!({
            "input": [
                { "type": "message", "role": "assistant", "content": [] },
                { "type": "message", "role": "assistant", "content": [] },
                { "type": "message", "role": "assistant", "content": [] }
            ]
        });

        let input = vec![
            ResponseItem::Message {
                id: Some("m1".into()),
                role: "assistant".into(),
                content: Vec::new(),
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: Some(String::new()),
                role: "assistant".into(),
                content: Vec::new(),
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
            ResponseItem::Message {
                id: None,
                role: "assistant".into(),
                content: Vec::new(),
                end_turn: None,
                phase: None,
                thought_signature: None,
            },
        ];

        attach_item_ids(&mut payload_json, &input);

        assert_eq!(payload_json["input"][0]["id"], json!("m1"));
        assert_eq!(payload_json["input"][1].get("id"), None);
        assert_eq!(payload_json["input"][2].get("id"), None);
    }

    #[test]
    fn attach_item_ids_noops_when_input_missing_or_not_array() {
        let input = vec![ResponseItem::Other];

        let mut without_input = json!({ "store": true });
        attach_item_ids(&mut without_input, &input);
        assert_eq!(without_input, json!({ "store": true }));

        let mut non_array_input = json!({ "input": { "type": "message" } });
        attach_item_ids(&mut non_array_input, &input);
        assert_eq!(non_array_input, json!({ "input": { "type": "message" } }));
    }
}
