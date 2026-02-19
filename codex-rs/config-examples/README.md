# Codex Configuration Examples

This directory contains example configuration files for Codex with multi-agent enhancements.

## Files

- `config.toml` - Main configuration file with all features enabled
- `config-pool.toml` - Configuration with API key pooling for rate limit management
- `auth.json` - Simple authentication file (single API key per provider)
- `auth-pool.json` - Authentication file with multiple API keys per provider

## Quick Start

1. Copy the example files to your Codex config directory:
   ```bash
   cp config-examples/config.toml ~/.codex/config.toml
   cp config-examples/auth.json ~/.codex/auth.json
   ```

2. Edit `~/.codex/auth.json` and add your API keys:
   ```json
   {
       "OPENAI_API_KEY": "sk-your-actual-key-here",
       "ANTHROPIC_API_KEY": "sk-ant-your-actual-key-here",
       "GEMINI_API_KEY": "AIza-your-actual-key-here",
       "XAI_API_KEY": "xai-your-actual-key-here"
   }
   ```

3. Adjust `~/.codex/config.toml` to your preferences

## Key Features in config.toml

### Multi-Agent System
```toml
[features]
multi_agent = true           # Enable multi-agent collaboration
agent_worktrees = true       # Isolated git worktrees for sub-agents
```

### Model Selection
```toml
model = "claude-sonnet-4-6"  # Default leader model
model_sub = "claude-sonnet-4-6"  # Sub-agent model (auto-selected if not set)
```

### Entire Integration
```toml
notify = ['entire', 'hooks', 'codex', 'notify']  # Enable Entire session tracking
```

### Memory System
```toml
[features]
memory_tool = true           # Enable memory extraction and consolidation
```

## API Key Pooling

For high-volume usage, use `config-pool.toml` and `auth-pool.json` to rotate between multiple API keys:

```toml
# In config-pool.toml
[model_providers.openai]
api_key_pool = [
    "OPENAI_API_KEY_POOL_1",
    "OPENAI_API_KEY_POOL_2",
    "OPENAI_API_KEY_POOL_3"
]
```

```json
// In auth-pool.json
{
  "OPENAI_API_KEY_POOL_1": "sk-key-1",
  "OPENAI_API_KEY_POOL_2": "sk-key-2",
  "OPENAI_API_KEY_POOL_3": "sk-key-3"
}
```

## MCP Servers

The example includes two MCP servers:

- `claude-code-mcp` - Claude Code integration
- `watermark-remover` - PDF watermark removal tool

## Custom Model Providers

```toml
[model_providers.codex]
base_url = 'https://your-api-endpoint.com/v1'
name = 'codex'
requires_openai_auth = true
wire_api = 'responses'
```

## Support

For more information, see the main repository documentation.
