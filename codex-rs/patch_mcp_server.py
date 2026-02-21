import re

with open('mcp-server/src/codex_tool_runner.rs', 'r') as f:
    content = f.read()

target = """                        // individual events in the future.
                    }
                }
            }"""

replacement = """                        // individual events in the future.
                    }
                    codex_core::protocol::EventMsg::FileSystemMutated(_) => {
                        // ignore for now in MCP
                    }
                }
            }"""

if "FileSystemMutated" not in content:
    content = content.replace(target, replacement)
    with open('mcp-server/src/codex_tool_runner.rs', 'w') as f:
        f.write(content)
