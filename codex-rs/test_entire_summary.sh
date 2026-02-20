#!/bin/bash
# Test script for Entire summary generation

set -e

echo "=== Entire Summary Feature Verification ==="
echo ""

# Step 1: Check if code compiles
echo "Step 1: Checking compilation..."
cd /Users/jqwang/01-agent/new-codex/codex-rs
cargo check -p codex-core 2>&1 | tail -5
echo "✓ Compilation check passed"
echo ""

# Step 2: Check if Entire is installed
echo "Step 2: Checking Entire CLI..."
if command -v entire &> /dev/null; then
    echo "✓ Entire CLI found: $(which entire)"
    entire --version || echo "  (version command not available)"
else
    echo "✗ Entire CLI not found. Install from: https://github.com/jiaqiwang969/cli"
    echo "  Run: cargo install --git https://github.com/jiaqiwang969/cli"
fi
echo ""

# Step 3: Check if we're in a git repo with Entire enabled
echo "Step 3: Checking Entire status in current repo..."
if [ -d .git ]; then
    echo "✓ Git repository detected"
    if entire status 2>/dev/null; then
        echo "✓ Entire is active in this repo"
    else
        echo "✗ Entire not initialized. Run: entire init"
    fi
else
    echo "✗ Not in a git repository"
fi
echo ""

# Step 4: Check for existing Entire checkpoints
echo "Step 4: Checking for Entire checkpoints..."
if [ -d .entire ]; then
    echo "✓ .entire directory exists"
    checkpoint_count=$(find .entire -name "*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
    echo "  Found $checkpoint_count checkpoint files"
    
    if [ -d .entire/summaries ]; then
        summary_count=$(find .entire/summaries -name "*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
        echo "  Found $summary_count summary files"
    else
        echo "  No .entire/summaries directory yet"
    fi
else
    echo "✗ No .entire directory found"
fi
echo ""

# Step 5: Show how to test summary generation
echo "Step 5: How to test summary generation"
echo "---------------------------------------"
echo ""
echo "To test the Entire summary feature:"
echo ""
echo "1. Make sure Entire is configured in ~/.codex/config.toml:"
echo "   [memories]"
echo "   entire_summary_enabled = true"
echo "   entire_summary_model = \"claude-3-5-haiku-20241022\"  # optional"
echo ""
echo "2. Make a code change and commit it:"
echo "   echo '// test' >> core/src/lib.rs"
echo "   git add core/src/lib.rs"
echo "   git commit -m 'test: verify entire summary'"
echo ""
echo "3. The Entire notify hook should create a checkpoint automatically"
echo ""
echo "4. Run Codex CLI and check context:"
echo "   codex"
echo "   # In the session, the context packet should include Entire summaries"
echo ""
echo "5. Check if summary was generated:"
echo "   ls -la .entire/summaries/"
echo "   cat .entire/summaries/<checkpoint-id>.json"
echo ""
echo "6. Verify summary content has these fields:"
echo "   - motivation: Why this change was made"
echo "   - approach: How it was implemented"
echo "   - challenges: What difficulties were encountered"
echo "   - tradeoffs: What compromises were made"
echo "   - outcome: What was achieved"
echo ""

# Step 6: Check config
echo "Step 6: Checking Codex config..."
if [ -f ~/.codex/config.toml ]; then
    echo "✓ Config file exists: ~/.codex/config.toml"
    if grep -q "entire_summary_enabled" ~/.codex/config.toml 2>/dev/null; then
        echo "✓ entire_summary_enabled found in config"
        grep "entire_summary" ~/.codex/config.toml | head -5
    else
        echo "✗ entire_summary_enabled not found in config"
        echo "  Add to [memories] section:"
        echo "  entire_summary_enabled = true"
    fi
else
    echo "✗ No config file at ~/.codex/config.toml"
fi
echo ""

echo "=== Verification Complete ==="
