# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Provider account pool

Codex can automatically fail over to another account within the *same* provider
when the current account fails. The provider is still selected by
`model_provider` (or by a profile), and only accounts in that provider's
`account_pool` are rotated.

Each account entry is a `(base_url, env_key)` pair:

```toml
model_provider = "openai-proxy"

[model_providers.openai-proxy]
name = "OpenAI Proxy"
wire_api = "responses"
account_pool = [
  { base_url = "https://api.vectorengine.ai/v1", env_key = "OPENAI_API_KEY_01" },
  { base_url = "https://api.vectorengine.ai/v1", env_key = "OPENAI_API_KEY_02" }
]

[profiles.gemini]
model_provider = "gemini-proxy"

[model_providers.gemini-proxy]
name = "Gemini Proxy"
wire_api = "gemini"
account_pool = [
  { base_url = "https://generativelanguage.googleapis.com/v1beta", env_key = "GEMINI_API_KEY_01" }
]
```

When a switch happens, Codex updates
`model_providers.<provider_id>.base_url` and `.env_key` in `config.toml`.
The account pool is tried in order, starting after the current account and
wrapping around until each account has been attempted at most once per turn.

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`. See the configuration reference for the latest MCP server options:

- https://developers.openai.com/codex/config-reference

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

Codex can run a notification hook when the agent finishes a turn. See the configuration reference for the latest notification settings:

- https://developers.openai.com/codex/config-reference

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
