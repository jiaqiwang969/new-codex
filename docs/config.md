# Configuration

For basic configuration instructions, see [this documentation](https://developers.openai.com/codex/config-basic).

For advanced configuration instructions, see [this documentation](https://developers.openai.com/codex/config-advanced).

For a full configuration reference, see [this documentation](https://developers.openai.com/codex/config-reference).

## Approvals reviewer

Approval handling now has two separate configuration axes:

- `approval_policy`: when Codex must escalate an action for approval
- `approvals_reviewer`: who reviews escalated actions (`user` or `guardian_subagent`)

The `smart_approvals` feature flag is only a rollout and UI gate. It does not
silently replace `approval_policy`.

Older configs that still use the deprecated `guardian_approval = true` alias are
migrated to `approvals_reviewer = "guardian_subagent"` when no explicit reviewer
is already set in the same scope.

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

When `account_pool` is present, pool order is the source of truth for account
selection. Every new turn starts scanning from the first pool entry. If an
account fails with an auth/rate-limit style error, Codex cools that account
down for 10 minutes for the current session, switches to the next pool entry
for the current turn, and retries the first account again on a later turn once
its cooldown has expired.

Codex does not rewrite `config.toml` or `config-pool.toml` to persist the last
successful account. If every account is still cooling down, Codex forces a
fresh probe from the first pool entry instead of failing immediately.

## Connecting to MCP servers

Codex can connect to MCP servers configured in `~/.codex/config.toml`. See the configuration reference for the latest MCP server options:

- https://developers.openai.com/codex/config-reference

For MCP tools that declare agent-context fields in their input schema, Codex can
auto-populate missing values at call time:

- `context`
- `workFolder` or `workdir`
- `memoryScopeVersion` or `memory_scope_version`
- `memoryScopeKind` or `memory_scope_kind`
- `memorySummarySha256` or `memory_summary_sha256`
- `memoryBindingKey` or `memory_binding_key`

Explicit values from the model are preserved. Codex only injects values when a
field is missing, null, or an empty string.

## MCP tool approvals

Codex stores per-tool approval overrides for custom MCP servers under
`mcp_servers` in `~/.codex/config.toml`:

```toml
[mcp_servers.docs.tools.search]
approval_mode = "approve"
```

## Apps (Connectors)

Use `$` in the composer to insert a ChatGPT connector; the popover lists accessible
apps. The `/apps` command lists available and installed apps. Connected apps appear first
and are labeled as connected; others are marked as can be installed.

## Notify

Codex can run a notification hook when the agent finishes a turn. See the configuration reference for the latest notification settings:

- https://developers.openai.com/codex/config-reference

When a notify hook is configured, Codex appends a legacy JSON payload as the last argv argument.
Current event types:

- `agent-turn-complete`: emitted after a turn completes.
- `mcp-tool-call-complete`: emitted after an MCP tool call finishes.

Both payloads include `provider-name`, `model-slug`, and (when available)
`memory-scope-version`, `memory-scope-kind`, `memory-summary-sha256`, and
`memory-binding-key`.
When the `memory_tool` feature is enabled,
payloads may also include a `memory-context` object with active memory scope metadata.

Codex also exports hook metadata via environment variables for easier integration:

- Common:
  `CODEX_HOOK_EVENT`, `CODEX_HOOK_THREAD_ID`, `CODEX_HOOK_TURN_ID`,
  `CODEX_HOOK_CWD`, `CODEX_HOOK_PROVIDER_NAME`, `CODEX_HOOK_MODEL_SLUG`
- Memory (when available):
  `CODEX_HOOK_MEMORY_SCOPE_VERSION`, `CODEX_HOOK_MEMORY_SCOPE_KIND`,
  `CODEX_HOOK_MEMORY_SUMMARY_SHA256`, `CODEX_HOOK_MEMORY_BINDING_KEY`,
  `CODEX_HOOK_ACTIVE_MEMORY_SCOPE_VERSION`, `CODEX_HOOK_ACTIVE_MEMORY_BINDING_KEY`
- MCP-only:
  `CODEX_HOOK_MCP_CALL_ID`, `CODEX_HOOK_MCP_SERVER`, `CODEX_HOOK_MCP_TOOL_NAME`,
  `CODEX_HOOK_MCP_STATUS`, `CODEX_HOOK_MCP_ERROR_MESSAGE`, `CODEX_HOOK_AGENT_NAME`

For `mcp-tool-call-complete`, `status` can be one of:
`ok`, `tool-error`, `transport-error`, `declined`, `cancelled`.

- `tool-error` means the MCP transport succeeded but the tool returned `is_error=true`.
- `transport-error` means the MCP call itself failed before a normal tool result was returned.
- `declined` means the user explicitly denied the approval prompt for the tool call.
- `cancelled` means the user cancelled the approval prompt or provided no usable answer.

When Codex knows which client started the turn, the legacy notify JSON payload
also includes a top-level `client` field. The TUI reports `codex-tui`, and the
app server reports the `clientInfo.name` value from `initialize`.

## JSON Schema

The generated JSON Schema for `config.toml` lives at `codex-rs/core/config.schema.json`.

## SQLite State DB

Codex stores the SQLite-backed state DB under `sqlite_home` (config key) or the
`CODEX_SQLITE_HOME` environment variable. When unset, WorkspaceWrite sandbox
sessions default to a temp directory; other modes default to `CODEX_HOME`.

## Custom CA Certificates

Codex can trust a custom root CA bundle for outbound HTTPS and secure websocket
connections when enterprise proxies or gateways intercept TLS. This applies to
login flows and to Codex's other external connections, including Codex
components that build reqwest clients or secure websocket clients through the
shared `codex-client` CA-loading path and remote MCP connections that use it.

Set `CODEX_CA_CERTIFICATE` to the path of a PEM file containing one or more
certificate blocks to use a Codex-specific CA bundle. If
`CODEX_CA_CERTIFICATE` is unset, Codex falls back to `SSL_CERT_FILE`. If
neither variable is set, Codex uses the system root certificates.

`CODEX_CA_CERTIFICATE` takes precedence over `SSL_CERT_FILE`. Empty values are
treated as unset.

The PEM file may contain multiple certificates. Codex also tolerates OpenSSL
`TRUSTED CERTIFICATE` labels and ignores well-formed `X509 CRL` sections in the
same bundle. If the file is empty, unreadable, or malformed, the affected Codex
HTTP or secure websocket connection reports a user-facing error that points
back to these environment variables.

## Notices

Codex stores "do not show again" flags for some UI prompts under the `[notice]` table.

## Plan mode defaults

`plan_mode_reasoning_effort` lets you set a Plan-mode-specific default reasoning
effort override. When unset, Plan mode uses the built-in Plan preset default
(currently `medium`). When explicitly set (including `none`), it overrides the
Plan preset. The string value `none` means "no reasoning" (an explicit Plan
override), not "inherit the global default". There is currently no separate
config value for "follow the global default in Plan mode".

## Realtime start instructions

`experimental_realtime_start_instructions` lets you replace the built-in
developer message Codex inserts when realtime becomes active. It only affects
the realtime start message in prompt history and does not change websocket
backend prompt settings or the realtime end/inactive message.

Ctrl+C/Ctrl+D quitting uses a ~1 second double-press hint (`ctrl + c again to quit`).
