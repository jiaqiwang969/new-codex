with open('tui/src/chatwidget.rs', 'r') as f:
    text = f.read()

import re

text = re.sub(
    r"EventMsg::ExecCommandEnd\(ev\) => self\.on_exec_command_end\(ev\),",
    "EventMsg::ExecCommandEnd(ev) => self.on_exec_command_end(ev),\n            EventMsg::FileSystemMutated(ev) => self.on_file_system_mutated(ev),",
    text
)

text = re.sub(
    r"    fn on_exec_command_end\(&mut self, ev: ExecCommandEndEvent\) \{",
    "    fn on_file_system_mutated(&mut self, ev: codex_core::protocol::FileSystemMutatedEvent) {\n        if !ev.files.is_empty() {\n            let mut txt = \"[Detected File Changes via Shell]\\n\".to_string();\n            for f in ev.files {\n                txt.push_str(&format!(\"  └ 📝 M {}\\n\", f));\n            }\n            self.on_agent_message(txt);\n        }\n    }\n\n    fn on_exec_command_end(&mut self, ev: ExecCommandEndEvent) {",
    text
)

with open('tui/src/chatwidget.rs', 'w') as f:
    f.write(text)
