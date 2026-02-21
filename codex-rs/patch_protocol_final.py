with open('protocol/src/protocol.rs', 'r') as f:
    text = f.read()

import re

text = text.replace(
    "#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]\npub struct DeprecationNoticeEvent {",
    "#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]\n#[serde(rename_all = \"camelCase\")]\n#[ts(export)]\npub struct FileSystemMutatedEvent {\n    pub call_id: String,\n    pub files: Vec<String>,\n}\n\n#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]\npub struct DeprecationNoticeEvent {"
)

with open('protocol/src/protocol.rs', 'w') as f:
    f.write(text)
