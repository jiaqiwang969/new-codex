import re

with open('core/src/git_side_effects.rs', 'r') as f:
    content = f.read()

content = content.replace("pub async fn track_tool_side_effects", "pub(crate) async fn track_tool_side_effects")

with open('core/src/git_side_effects.rs', 'w') as f:
    f.write(content)
