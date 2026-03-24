use crate::auth::AuthMode;
use codex_protocol::openai_models::InputModality;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelUpgrade;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::openai_models::default_input_modalities;
use indoc::indoc;
use once_cell::sync::Lazy;

pub const HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG: &str = "hide_gpt5_1_migration_prompt";
pub const HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG: &str =
    "hide_gpt-5.1-codex-max_migration_prompt";

pub(crate) static PRESETS: Lazy<Vec<ModelPreset>> = Lazy::new(|| {
    vec![
        ModelPreset {
            id: "gpt-5.3-codex".to_string(),
            model: "gpt-5.3-codex".to_string(),
            display_name: "gpt-5.3-codex".to_string(),
            description: "Latest frontier agentic coding model with enhanced reasoning.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: true,
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5.3-codex-spark|[pro]".to_string(),
            model: "gpt-5.3-codex-spark|[pro]".to_string(),
            display_name: "gpt-5.3-codex-spark|[pro]".to_string(),
            description:
                "Realtime coding model optimized for low-latency edits (text only, Pro, 128K context)."
                    .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: true,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text],
        },
        ModelPreset {
            id: "gpt-5.2-codex".to_string(),
            model: "gpt-5.2-codex".to_string(),
            display_name: "gpt-5.2-codex".to_string(),
            description: "Frontier agentic coding model.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: true,
            is_default: false,
            upgrade: Some(gpt_53_codex_upgrade()),
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5.2".to_string(),
            model: "gpt-5.2".to_string(),
            display_name: "gpt-5.2".to_string(),
            description: "Latest frontier model with improvements across knowledge, reasoning and coding".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "bengalfox".to_string(),
            model: "bengalfox".to_string(),
            display_name: "bengalfox".to_string(),
            description: "bengalfox".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: true,
            is_default: false,
            upgrade: None,
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "boomslang".to_string(),
            model: "boomslang".to_string(),
            display_name: "boomslang".to_string(),
            description: "boomslang".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Extra high reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        // Deprecated models.
        ModelPreset {
            id: "gemini-3.1-pro-preview".to_string(),
            model: "gemini-3.1-pro-preview".to_string(),
            display_name: "Gemini 3.1 Pro".to_string(),
            description: "Google Gemini 3.1 Pro with enhanced reasoning and 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gemini-3-pro-preview".to_string(),
            model: "gemini-3-pro-preview".to_string(),
            display_name: "Gemini 3 Pro".to_string(),
            description: "Google Gemini 3 Pro with deep reasoning and 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gemini-3-flash-preview".to_string(),
            model: "gemini-3-flash-preview".to_string(),
            display_name: "Gemini 3 Flash".to_string(),
            description: "Google Gemini 3 Flash — fast and efficient with 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gemini-3.1-flash-image-preview".to_string(),
            model: "gemini-3.1-flash-image-preview".to_string(),
            display_name: "Gemini 3.1 Flash Image".to_string(),
            description:
                "Gemini 3.1 Flash Image for text, image understanding, and image generation."
                    .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description:
                    "Default Gemini reasoning behaviour for image workflows.".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gemini-3-pro-image-preview".to_string(),
            model: "gemini-3-pro-image-preview".to_string(),
            display_name: "Gemini 3 Pro Image".to_string(),
            description: "Gemini 3 Pro for text, image understanding, and image generation."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description:
                    "Default Gemini reasoning behaviour for image workflows.".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gemma-3n".to_string(),
            model: "gemma-3n".to_string(),
            display_name: "Gemma 3n (Local)".to_string(),
            description: "Local Gemma 3n served via Gemini-compatible API.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks"
                        .to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "grok-4-latest".to_string(),
            model: "grok-4-latest".to_string(),
            display_name: "Grok 4 Latest".to_string(),
            description: "xAI Grok 4 via OpenAI-compatible Responses API (256K context)."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::None,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::None,
                description: "Reasoning effort is not configurable on this model.".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "grok-4-1-fast-reasoning".to_string(),
            model: "grok-4-1-fast-reasoning".to_string(),
            display_name: "Grok 4.1 Fast Reasoning".to_string(),
            description:
                "xAI Grok 4.1 fast reasoning via OpenAI-compatible Responses API (2M context)."
                    .to_string(),
            default_reasoning_effort: ReasoningEffort::None,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::None,
                description: "Reasoning effort is not configurable on this model.".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "claude-opus-4-6".to_string(),
            model: "claude-opus-4-6".to_string(),
            display_name: "Claude Opus 4.6".to_string(),
            description: "Anthropic Claude Opus 4.6 — deep reasoning with 1M context."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced reasoning for general tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Deep reasoning for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Maximum reasoning depth with extended thinking".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text, InputModality::Image],
        },
        ModelPreset {
            id: "claude-sonnet-4-6".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            display_name: "Claude Sonnet 4.6".to_string(),
            description: "Anthropic Claude Sonnet 4.6 — fast execution with 1M context."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text, InputModality::Image],
        },
        ModelPreset {
            id: "claude-haiku-4-5-20251001".to_string(),
            model: "claude-haiku-4-5-20251001".to_string(),
            display_name: "Claude Haiku 4.5".to_string(),
            description: "Anthropic Claude Haiku 4.5 — fastest responses with 200K context."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Low,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses optimized for speed".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text, InputModality::Image],
        },
        // Antigravity Gemini models
        ModelPreset {
            id: "antigravity/gemini-3.1-flash-image-preview".to_string(),
            model: "antigravity/gemini-3.1-flash-image-preview".to_string(),
            display_name: "Antigravity Gemini 3.1 Flash Image".to_string(),
            description: "Gemini 3.1 Flash Image via CLIProxyAPI for text, image understanding, and image generation."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Medium,
                description:
                    "Default Gemini reasoning behaviour for image workflows.".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gemini-3.1-pro-preview".to_string(),
            model: "antigravity/gemini-3.1-pro-preview".to_string(),
            display_name: "Antigravity Gemini 3.1 Pro".to_string(),
            description: "Gemini 3.1 Pro via CLIProxyAPI with full thoughtSignature support and 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gemini-3-pro-preview".to_string(),
            model: "antigravity/gemini-3-pro-preview".to_string(),
            display_name: "Antigravity Gemini 3 Pro".to_string(),
            description: "Gemini 3 Pro via CLIProxyAPI with deep reasoning and 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gemini-3-flash-preview".to_string(),
            model: "antigravity/gemini-3-flash-preview".to_string(),
            display_name: "Antigravity Gemini 3 Flash".to_string(),
            description: "Gemini 3 Flash via CLIProxyAPI — fast and efficient with 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balances speed and reasoning depth for everyday tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gemini-2.5-flash".to_string(),
            model: "antigravity/gemini-2.5-flash".to_string(),
            display_name: "Antigravity Gemini 2.5 Flash".to_string(),
            description: "Gemini 2.5 Flash via CLIProxyAPI — ultra-fast with 1M context.".to_string(),
            default_reasoning_effort: ReasoningEffort::Low,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses optimized for speed".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gemini-2.5-flash-lite".to_string(),
            model: "antigravity/gemini-2.5-flash-lite".to_string(),
            display_name: "Antigravity Gemini 2.5 Flash Lite".to_string(),
            description: "Gemini 2.5 Flash Lite via CLIProxyAPI — lowest-latency Gemini path."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Low,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses optimized for speed".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "antigravity/gpt-oss-120b-medium".to_string(),
            model: "antigravity/gpt-oss-120b-medium".to_string(),
            display_name: "Antigravity GPT-OSS 120B Medium".to_string(),
            description: "GPT-OSS 120B Medium via CLIProxyAPI for general text and coding tasks."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text],
        },
        ModelPreset {
            id: "antigravity/tab_flash_lite_preview".to_string(),
            model: "antigravity/tab_flash_lite_preview".to_string(),
            display_name: "Antigravity Tab Flash Lite Preview".to_string(),
            description: "Tab Flash Lite Preview via CLIProxyAPI for ultra-fast text completions."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Low,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Low,
                description: "Fast responses optimized for latency".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text],
        },
        ModelPreset {
            id: "antigravity/tab_jump_flash_lite_preview".to_string(),
            model: "antigravity/tab_jump_flash_lite_preview".to_string(),
            display_name: "Antigravity Tab Jump Flash Lite Preview".to_string(),
            description:
                "Tab Jump Flash Lite Preview via CLIProxyAPI for low-latency text workflows."
                    .to_string(),
            default_reasoning_effort: ReasoningEffort::Low,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffort::Low,
                description: "Fast responses optimized for latency".to_string(),
            }],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text],
        },
        // Antigravity Anthropic models
        ModelPreset {
            id: "antigravity/claude-sonnet-4-6".to_string(),
            model: "antigravity/claude-sonnet-4-6".to_string(),
            display_name: "Antigravity Claude Sonnet 4.6".to_string(),
            description: "Claude Sonnet 4.6 via CLIProxyAPI — fast execution with 1M context."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fast responses with lighter reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced speed and reasoning depth".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Greater reasoning depth for complex problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text, InputModality::Image],
        },
        ModelPreset {
            id: "antigravity/claude-opus-4-6-thinking".to_string(),
            model: "antigravity/claude-opus-4-6-thinking".to_string(),
            display_name: "Antigravity Claude Opus 4.6 Thinking".to_string(),
            description: "Claude Opus 4.6 Thinking via CLIProxyAPI — deep reasoning with extended thinking and 1M context."
                .to_string(),
            default_reasoning_effort: ReasoningEffort::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Balanced reasoning for general tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Deep reasoning for complex problems".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::XHigh,
                    description: "Maximum reasoning depth with extended thinking".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            input_modalities: vec![InputModality::Text, InputModality::Image],
        },
        // Deprecated models.
        ModelPreset {
            id: "gpt-5-codex".to_string(),
            model: "gpt-5-codex".to_string(),
            display_name: "gpt-5-codex".to_string(),
            description: "Optimized for codex.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fastest responses with limited reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Dynamically adjusts reasoning based on the task".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5-codex-mini".to_string(),
            model: "gpt-5-codex-mini".to_string(),
            display_name: "gpt-5-codex-mini".to_string(),
            description: "Optimized for codex. Cheaper, faster, but less capable.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Dynamically adjusts reasoning based on the task".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5.1-codex".to_string(),
            model: "gpt-5.1-codex".to_string(),
            display_name: "gpt-5.1-codex".to_string(),
            description: "Optimized for codex.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Fastest responses with limited reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Dynamically adjusts reasoning based on the task".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems"
                        .to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5".to_string(),
            model: "gpt-5".to_string(),
            display_name: "gpt-5".to_string(),
            description: "Broad world knowledge with strong general reasoning.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Minimal,
                    description: "Fastest responses with little reasoning".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
        ModelPreset {
            id: "gpt-5.1".to_string(),
            model: "gpt-5.1".to_string(),
            display_name: "gpt-5.1".to_string(),
            description: "Broad world knowledge with strong general reasoning.".to_string(),
            default_reasoning_effort: ReasoningEffort::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Low,
                    description: "Balances speed with some reasoning; useful for straightforward queries and short explanations".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::Medium,
                    description: "Provides a solid balance of reasoning depth and latency for general-purpose tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffort::High,
                    description: "Maximizes reasoning depth for complex or ambiguous problems".to_string(),
                },
            ],
            supports_personality: false,
            is_default: false,
            upgrade: Some(gpt_52_codex_upgrade()),
            show_in_picker: false,
            supported_in_api: true,
            input_modalities: default_input_modalities(),
        },
    ]
});

fn gpt_53_codex_upgrade() -> ModelUpgrade {
    ModelUpgrade {
        id: "gpt-5.3-codex".to_string(),
        reasoning_effort_mapping: None,
        migration_config_key: "gpt-5.3-codex".to_string(),
        model_link: None,
        upgrade_copy: Some(
            "Codex is now powered by gpt-5.3-codex, the latest frontier agentic coding model with enhanced reasoning."
                .to_string(),
        ),
        migration_markdown: Some(
            indoc! {r#"
                **Codex just got an upgrade. Introducing {model_to}.**

                Codex is now powered by gpt-5.3-codex with enhanced reasoning capabilities. You can continue using {model_from} if you prefer.
            "#}
            .to_string(),
        ),
    }
}

fn gpt_52_codex_upgrade() -> ModelUpgrade {
    ModelUpgrade {
        id: "gpt-5.2-codex".to_string(),
        reasoning_effort_mapping: None,
        migration_config_key: "gpt-5.2-codex".to_string(),
        model_link: Some("https://openai.com/index/introducing-gpt-5-2-codex".to_string()),
        upgrade_copy: Some(
            "Codex is now powered by gpt-5.2-codex, our latest frontier agentic coding model. It is smarter and faster than its predecessors and capable of long-running project-scale work."
                .to_string(),
        ),
        migration_markdown: Some(
            indoc! {r#"
                **Codex just got an upgrade. Introducing {model_to}.**

                Codex is now powered by gpt-5.2-codex, our latest frontier agentic coding model. It is smarter and faster than its predecessors and capable of long-running project-scale work. Learn more about {model_to} at https://openai.com/index/introducing-gpt-5-2-codex

                You can continue using {model_from} if you prefer.
            "#}
            .to_string(),
        ),
    }
}

pub fn builtin_model_presets(_auth_mode: Option<AuthMode>) -> Vec<ModelPreset> {
    PRESETS.iter().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn only_one_default_model_is_configured() {
        let default_models = PRESETS.iter().filter(|preset| preset.is_default).count();
        assert!(default_models == 1);
    }

    #[test]
    fn gemma_and_grok_are_visible_in_picker() {
        let gemma = PRESETS
            .iter()
            .find(|preset| preset.model == "gemma-3n")
            .expect("gemma preset should exist");
        let grok = PRESETS
            .iter()
            .find(|preset| preset.model == "grok-4-latest")
            .expect("grok preset should exist");
        let spark = PRESETS
            .iter()
            .find(|preset| preset.model == "gpt-5.3-codex-spark|[pro]")
            .expect("spark preset should exist");

        assert!(gemma.show_in_picker);
        assert_eq!(gemma.default_reasoning_effort, ReasoningEffort::Medium);
        assert!(grok.show_in_picker);
        assert_eq!(grok.default_reasoning_effort, ReasoningEffort::None);
        assert!(spark.show_in_picker);
        assert_eq!(spark.default_reasoning_effort, ReasoningEffort::Medium);
    }

    #[test]
    fn claude_models_are_visible_in_picker() {
        let opus = PRESETS
            .iter()
            .find(|preset| preset.model == "claude-opus-4-6")
            .expect("claude opus preset should exist");
        let sonnet = PRESETS
            .iter()
            .find(|preset| preset.model == "claude-sonnet-4-6")
            .expect("claude sonnet preset should exist");
        let haiku = PRESETS
            .iter()
            .find(|preset| preset.model == "claude-haiku-4-5-20251001")
            .expect("claude haiku preset should exist");

        assert!(opus.show_in_picker);
        assert_eq!(opus.default_reasoning_effort, ReasoningEffort::High);
        assert!(sonnet.show_in_picker);
        assert_eq!(sonnet.default_reasoning_effort, ReasoningEffort::Medium);
        assert!(haiku.show_in_picker);
        assert_eq!(haiku.default_reasoning_effort, ReasoningEffort::Low);
    }

    #[test]
    fn antigravity_gemini_models_use_public_slugs_and_reasoning_effort() {
        let gemini_31_pro = PRESETS
            .iter()
            .find(|preset| preset.model == "antigravity/gemini-3.1-pro-preview")
            .expect("antigravity gemini 3.1 pro preset should exist");
        let gemini_3_pro = PRESETS
            .iter()
            .find(|preset| preset.model == "antigravity/gemini-3-pro-preview")
            .expect("antigravity gemini 3 pro preset should exist");
        let gemini_3_flash = PRESETS
            .iter()
            .find(|preset| preset.model == "antigravity/gemini-3-flash-preview")
            .expect("antigravity gemini 3 flash preset should exist");

        for removed_slug in [
            "antigravity/gemini-3.1-pro-high",
            "antigravity/gemini-3.1-pro-low",
            "antigravity/gemini-3-pro-high",
            "antigravity/gemini-3-flash",
        ] {
            assert!(
                PRESETS.iter().all(|preset| preset.model != removed_slug),
                "{removed_slug} should not remain in presets",
            );
        }

        assert!(gemini_31_pro.show_in_picker);
        assert_eq!(
            gemini_31_pro.default_reasoning_effort,
            ReasoningEffort::High
        );
        assert_eq!(
            gemini_31_pro
                .supported_reasoning_efforts
                .iter()
                .map(|preset| preset.effort)
                .collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ]
        );

        assert!(gemini_3_pro.show_in_picker);
        assert_eq!(gemini_3_pro.default_reasoning_effort, ReasoningEffort::High);
        assert_eq!(
            gemini_3_pro
                .supported_reasoning_efforts
                .iter()
                .map(|preset| preset.effort)
                .collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ]
        );

        assert!(gemini_3_flash.show_in_picker);
        assert_eq!(
            gemini_3_flash.default_reasoning_effort,
            ReasoningEffort::Medium
        );
        assert_eq!(
            gemini_3_flash
                .supported_reasoning_efforts
                .iter()
                .map(|preset| preset.effort)
                .collect::<Vec<_>>(),
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
            ]
        );
    }

    #[test]
    fn gemini_image_presets_keep_only_supported_antigravity_preview_models() {
        for preview_slug in [
            "gemini-3.1-flash-image-preview",
            "gemini-3-pro-image-preview",
            "antigravity/gemini-3.1-flash-image-preview",
        ] {
            let preset = PRESETS
                .iter()
                .find(|preset| preset.model == preview_slug)
                .unwrap_or_else(|| panic!("{preview_slug} preset should exist"));

            assert!(
                preset.show_in_picker,
                "{preview_slug} should appear in picker"
            );
            assert_eq!(
                preset.default_reasoning_effort,
                ReasoningEffort::Medium,
                "{preview_slug} should default to medium reasoning",
            );
            assert_eq!(
                preset
                    .supported_reasoning_efforts
                    .iter()
                    .map(|preset| preset.effort)
                    .collect::<Vec<_>>(),
                vec![ReasoningEffort::Medium],
                "{preview_slug} should only support medium reasoning",
            );
        }

        for removed_slug in [
            "gemini-3.1-flash-image",
            "gemini-3-pro-image",
            "antigravity/gemini-3.1-flash-image",
            "antigravity/gemini-3-pro-image",
            "antigravity/gemini-3-pro-image-preview",
        ] {
            assert!(
                PRESETS.iter().all(|preset| preset.model != removed_slug),
                "{removed_slug} should not remain in presets",
            );
        }
    }
}
