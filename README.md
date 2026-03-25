  这个项目是基于 OpenAI Codex CLI 的深度定制 fork，在上游基础上扩展了 6 大核心能力：

  ---
  1. 多模型 Provider 支持

  - OpenAI (GPT-5.3-codex-spark|[pro], GPT-5.3-codex, GPT-5.2-codex)
  - Google Gemini (Gemini 3 Pro/Flash, Gemini 3 Pro Image, 1M context)
  - Grok (Grok 4, Grok 4.1 Fast Reasoning)
  - 本地模型 (Gemma-3n via Ollama/LM Studio)
  - 运行时自动切换 + Account Pool 故障转移 (400/401/403/429 自动换号，支持多轮循环)
  - 模型兼容矩阵：按模型自动启用/禁用 web search、reasoning effort、image 等能力

  2. 跨会话记忆系统

  - SQLite 持久化的 Thread Memory（对话摘要 + trace）
  - Context Packet 注入：自动将记忆、用户指令、项目 memories 注入 MCP 调用和子 agent
  - 会话压缩后自动更新记忆，最小 10 分钟间隔防抖

  3. 多 Agent 协作

  - spawn_agent / send_input / resume_agent / wait / close_agent 完整生命周期
  - Agent Worktrees：每个子 agent 在隔离的 git worktree 中工作
  - Lease 持久化：.codex/leases/ 目录，支持 worktree 恢复
  - Context Packet 自动注入子 agent 启动 prompt

  4. 图像处理流水线

  - /ref-image：设置参考图片
  - /ref-image-batch：批量处理文件夹中的图片
  - /image-quality (1K/2K/4K) + /aspect-ratio (1:1/16:9/9:16/4:3/3:4)
  - /pdf-update：PDF 转图片 + 批量图片处理
  - Gemini Image 模型专用配置（response_modalities、image_config）

  5. TUI 增强

  - Git Graph (Ctrl+G)：内嵌 git 提交历史可视化，Unicode 圆角风格
  - Session Bar (Ctrl+P)：tmux 风格底部面板，会话导航/新建/删除/重命名
  - 后台预热：2s 空闲后自动加载 session 列表和 git graph，打开时即时显示
  - Ralph Loop (/ralph-loop)：迭代自纠正循环，支持 <promise> 完成检测

  6. 基础设施

  - Gemma 系统 prompt 截断（29K→4K，防止本地小模型空输出）
  - Thought Signature 跨模型泄漏防护（Gemini→GPT 切换时自动清理）
  - WebSocket preconnect 跨 turn 复用
  - MCP 后台初始化（不阻塞启动）
  - Debug CLI：thread-memory backfill、agent-worktrees list/restore

  ---
  简单来说：从单一 OpenAI 模型的 CLI 工具，扩展成了支持 4 家 Provider、跨会话记忆、多 Agent
  协作、图像处理、git 可视化的多模型 AI 开发平台。

  详细的技术文档、代码组织、配置说明和 MCP 用法请参阅 [codex-rs/README.md](./codex-rs/README.md)。

  当前 merge 方向不再继续维护旧的 `/freeze`、`endpoint-sec`、本地 Smart Access 安全放行链路。
  这几条线已降级为历史尝试；安全审批能力以官方 `approval_policy` +
  `approvals_reviewer` / guardian 语义为准，账号池、多模型路由、记忆、TUI 工作台增强等定制能力继续保留。
