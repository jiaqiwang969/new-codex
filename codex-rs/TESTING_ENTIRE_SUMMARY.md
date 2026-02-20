# Testing Entire Summary Feature

## Prerequisites

1. **Entire CLI installed**
   ```bash
   cargo install --git https://github.com/jiaqiwang969/cli
   ```

2. **Codex built with the new changes**
   ```bash
   cd /Users/jqwang/01-agent/new-codex/codex-rs
   cargo build --release -p codex-cli
   ```

3. **Config updated**
   Add to `~/.codex/config.toml`:
   ```toml
   [memories]
   entire_summary_enabled = true
   entire_summary_model = "claude-3-5-haiku-20241022"
   ```

## Test Scenario: Integration Test with Real Codex Session

### Step 1: Initialize a test repository

```bash
mkdir -p /tmp/entire-summary-test
cd /tmp/entire-summary-test
git init
git config user.name "Test User"
git config user.email "test@example.com"
entire enable
echo "# Test Project" > README.md
git add README.md
git commit -m "Initial commit"
```

### Step 2: Configure Entire notify hook

Check if `~/.codex/config.toml` has:
```toml
notify = ["entire", "hooks", "codex", "notify"]
```

### Step 3: Run a Codex session and verify

```bash
cd /tmp/entire-summary-test
codex
# Make a change, then check:
ls -la .entire/summaries/
```

## Debugging

### Check logs
```bash
tail -f ~/.codex/logs/codex.log | grep -i "entire\|summary"
```

### Common issues
1. Summary not generated - check config and model access
2. Checkpoint created but no summary - wait for async task
3. Summary not in context - verify entire_summary_enabled

## Success Criteria

- Code compiles without errors
- Checkpoints created after Codex turns
- Summary files in `.entire/summaries/`
- Summaries have correct JSON structure
- Summaries loaded into context
