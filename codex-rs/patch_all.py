import re

# 1. git_info.rs
with open('core/src/git_info.rs', 'r') as f:
    git_info = f.read()
git_info = git_info.replace(
    "async fn run_git_command_with_timeout(",
    "pub(crate) async fn run_git_command_with_timeout("
)
with open('core/src/git_info.rs', 'w') as f:
    f.write(git_info)

# 2. codex.rs
with open('core/src/codex.rs', 'r') as f:
    codex = f.read()

# Fix TurnContext creations
codex = re.sub(
    r"(turn_metadata_state: Arc::new\(TurnMetadataState::default\(\)\)),\n\s*\}",
    r"\1,\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),\n        }",
    codex
)

codex = re.sub(
    r"(Self \{\n\s*sub_id),\n",
    r"\1,\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),\n",
    codex
)

codex = re.sub(
    r"(let review_turn_context = TurnContext \{\n\s*sub_id: uuid::Uuid::new_v4\(\)\.to_string\(\)),",
    r"\1,\n            side_effects_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::BTreeSet::new())),",
    codex
)

with open('core/src/codex.rs', 'w') as f:
    f.write(codex)

# 3. policy.rs
with open('core/src/rollout/policy.rs', 'r') as f:
    policy = f.read()

policy = policy.replace(
    "| EventMsg::CollabResumeBegin(_) => None,",
    "| EventMsg::CollabResumeBegin(_)\n        | EventMsg::FileSystemMutated(_) => None,"
)

with open('core/src/rollout/policy.rs', 'w') as f:
    f.write(policy)

