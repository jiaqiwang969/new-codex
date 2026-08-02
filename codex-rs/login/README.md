# Codex 多账号认证 Profile 扩展

> `CODEX_AUTH_PROFILE` 是本 Codex fork 的本地扩展，不是当前上游 Codex 的公开配置项。

## 背景

上游 Codex 会把登录凭据缓存在 `$CODEX_HOME/auth.json`（默认
`~/.codex/auth.json`）或操作系统凭据存储中，并在运行期间自动刷新 OAuth
token。参见官方的 [Login caching](https://learn.chatgpt.com/docs/auth#login-caching)
与 [Credential storage](https://learn.chatgpt.com/docs/auth#credential-storage)。

轮流把历史 `auth-*.json` 覆盖成 `auth.json` 不能可靠地切换账号：

- token 的有效性由服务端决定；复制已吊销的 token 仍会得到
  `HTTP 401 token_revoked`。
- 运行中的 Codex 会缓存认证快照，外部换文件不会让该进程安全地热切换身份。
- 多个账号共写一个文件时，登录、退出和 refresh token 轮换可能互相覆盖。
- `codex_apps` MCP 初始化使用当前进程的 token；旧 token 或缺少 connector 权限时，
  MCP 握手会失败。

本扩展不改变 OAuth 协议，只在认证存储层增加“每个进程选择一个认证槽”的能力。

## 目标与映射

未设置 `CODEX_AUTH_PROFILE` 时保持原行为；命名 profile 只隔离认证：

| 启动环境 | 文件认证槽 |
|---|---|
| 未设置 | `$CODEX_HOME/auth.json` |
| `CODEX_AUTH_PROFILE=jiaqiwang969` | `$CODEX_HOME/auth-jiaqiwang969.json` |
| `CODEX_AUTH_PROFILE=OmarGuthorn8` | `$CODEX_HOME/auth-OmarGuthorn8.json` |
| `CODEX_AUTH_PROFILE=zhiyingzhong969` | `$CODEX_HOME/auth-zhiyingzhong969.json` |

`config.toml`、sessions、history、memories、SQLite、skills、plugins 和 MCP 配置仍由
同一个 `CODEX_HOME` 提供，因此继续共享。

## Profile 名称

合法名称必须匹配：

```text
[A-Za-z0-9][A-Za-z0-9._-]{0,63}
```

名称长度为 1–64 个 ASCII 字符。空字符串、路径分隔符、空格、非 ASCII 字符、非法
首字符或超长名称都会 fail closed：认证的读取、写入和删除返回错误，绝不回退到
默认 `auth.json`。

代码保留名称大小写；但 macOS 默认文件系统通常不区分大小写，因此不要把仅大小写
不同的名称（例如 `Foo` 和 `foo`）当成两个独立槽。恢复默认槽应取消变量：

```bash
unset CODEX_AUTH_PROFILE
```

## 实现原理

认证存储使用统一的 `AuthStorageIdentity`：

```text
AuthStorageIdentity
├── codex_home
└── profile
    ├── Default
    ├── Named("<name>")
    └── Invalid
```

进程第一次使用认证存储时捕获 `CODEX_AUTH_PROFILE`，之后同一进程中的 load、save、
refresh、revoke、login、logout 和 delete 始终使用同一个槽。进程内后续修改环境变量
不会改变认证身份。

各存储后端都使用同一 identity：

| 后端 | 隔离方式 |
|---|---|
| File | `auth.json` 或 `auth-<profile>.json` |
| Direct Keyring | profile 参与稳定的 keyring store key 计算 |
| Secrets Keyring | 命名 profile 使用 `$CODEX_HOME/auth-profiles/<profile>` 作为独立 storage home |
| Auto | Keyring 主存储和文件 fallback 共用同一 profile |
| Ephemeral | 内存 Map 的 key 包含 profile identity |

Secrets Keyring 不把多个账号塞进同一个加密文件，避免并发整文件写入造成 lost update。
未设置环境变量时，默认文件路径、默认 keyring key 和 Secrets namespace 均保持不变。

CLI doctor 通过 `codex_login::active_auth_file()` 显示当前进程选择的文件/fallback 路径；
非法 profile 会显示明确错误，而不会误报默认账号。

## 添加和运行账号

不要复制另一个账号的认证文件。每个新 profile 首次使用时应获得独立 OAuth grant：

```bash
CODEX_AUTH_PROFILE=zhiyingzhong969 codex login
```

在浏览器中确认正确的 ChatGPT 账号和 workspace。若需要 `codex_apps`/connector，优先
使用普通浏览器登录；本 fork 的标准浏览器授权请求包括
`api.connectors.read` 与 `api.connectors.invoke`。

登录后启动：

```bash
CODEX_AUTH_PROFILE=zhiyingzhong969 codex
```

多个账号可以并存：

```bash
env -u CODEX_AUTH_PROFILE codex
CODEX_AUTH_PROFILE=jiaqiwang969 codex
CODEX_AUTH_PROFILE=OmarGuthorn8 codex
CODEX_AUTH_PROFILE=zhiyingzhong969 codex
```

`codex login status` 只确认本地凭据可读取及其认证方式，不会向服务端证明 token 尚未
吊销。若服务请求返回 `token_revoked`，应在对应 profile 下重新进行浏览器登录，然后
启动一个新的同 profile Codex 进程。

## 并发语义与限制

- 已运行且未设置 profile 的旧进程继续使用默认认证快照，不必为了新增账号退出。
- 本扩展不支持在同一进程内热切换账号；切换账号应启动另一个进程。
- 多个进程使用同一 profile 时仍共享一份凭据，不应并发执行该 profile 的 login/logout。
- 不要复制同一 refresh token 到多个 profile，否则 token 轮换仍会竞争。
- 非认证状态仍然共享；不要让两个账号同时恢复并写入同一条具体 session/rollout。
- 一个 profile 的 logout 只清理自己的文件、Keyring 或 Secrets 凭据，不清理其他槽。

## MCP `token_revoked`

典型错误：

```text
MCP client for `codex_apps` failed to start
HTTP 401
token_revoked
Encountered invalidated oauth token for user
```

这表示服务端拒绝当前 OAuth token，不是 JSON 文件名解析问题。处理方式：

```bash
CODEX_AUTH_PROFILE=<profile> codex login
CODEX_AUTH_PROFILE=<profile> codex
```

重新登录一个命名 profile 不要求退出其他账号的进程；但已经启动失败的 MCP 客户端
不会因外部换文件而可靠地恢复，应启动新的同 profile 进程。

## 安全

所有 `auth*.json` 都包含敏感 token，应像密码一样处理：

- 不要提交到 Git，也不要放进 diff、patch、issue、聊天或日志。
- 不要把其他账号的文件当作新 profile 模板。
- 备份旧凭据时仍需保持秘密和严格文件权限。
- 本扩展不会绕过 workspace、RBAC、订阅、管理员策略或服务端吊销规则。

## 相关源码与验证

- `src/auth/storage.rs`：profile 解析、进程级捕获和全部存储后端隔离。
- `src/auth/storage_tests.rs`：语法、兼容、隔离和 fail-closed 测试。
- `src/auth/mod.rs`、`src/lib.rs`：公开活动认证文件接口。
- `../cli/src/doctor.rs`：profile-aware doctor 输出。

本次实现的本地验证结果：

```text
codex-login: 162 / 162 passed
codex-cli:   300 / 300 passed
profile 环境污染回归测试: passed
release build: succeeded
```

验证没有执行真实账号的 login/logout，没有把真实 token 写入测试或交付物，也没有终止
现有 Codex 进程。
