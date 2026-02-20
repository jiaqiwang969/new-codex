#!/bin/bash
# End-to-end test for Entire Summary feature

set -e

echo "=== Entire Summary E2E Test ==="
echo ""

# Step 1: Build Codex with new changes
echo "Step 1: Building Codex CLI..."
cd /Users/jqwang/01-agent/new-codex/codex-rs
cargo build --release -p codex-cli 2>&1 | tail -3
echo "✓ Build complete"
echo ""

# Step 2: Create test repository
echo "Step 2: Creating test repository..."
TEST_DIR="/tmp/entire-summary-test-$(date +%s)"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR"

git init
git config user.name "Test User"
git config user.email "test@example.com"

echo "# Test Project for Entire Summary" > README.md
git add README.md
git commit -m "Initial commit"
echo "✓ Test repo created at: $TEST_DIR"
echo ""

# Step 3: Enable Entire
echo "Step 3: Enabling Entire..."
if command -v entire &> /dev/null; then
    entire enable
    echo "✓ Entire enabled"
else
    echo "✗ Entire CLI not found. Install with:"
    echo "  cargo install --git https://github.com/jiaqiwang969/cli"
    exit 1
fi
echo ""

# Step 4: Verify config
echo "Step 4: Verifying Codex config..."
if grep -q "entire_summary_enabled = true" ~/.codex/config.toml; then
    echo "✓ entire_summary_enabled = true"
else
    echo "✗ Config not set correctly"
    exit 1
fi

if grep -q "entire_summary_model" ~/.codex/config.toml; then
    MODEL=$(grep "entire_summary_model" ~/.codex/config.toml | cut -d'"' -f2)
    echo "✓ entire_summary_model = \"$MODEL\""
else
    echo "⚠ No explicit model set, will use model_sub fallback"
fi
echo ""

# Step 5: Instructions for manual testing
echo "Step 5: Manual Testing Instructions"
echo "===================================="
echo ""
echo "The test repository is ready at:"
echo "  $TEST_DIR"
echo ""
echo "To test the Entire summary feature:"
echo ""
echo "1. Start Codex in the test directory:"
echo "   cd $TEST_DIR"
echo "   /Users/jqwang/01-agent/new-codex/codex-rs/target/release/codex"
echo ""
echo "2. In the Codex session, ask it to create a simple Python script:"
echo "   'Create a hello.py script that prints Hello World'"
echo ""
echo "3. After the session completes, check for Entire checkpoint:"
echo "   ls -la .entire/checkpoints/"
echo ""
echo "4. Wait a few seconds for async summary generation:"
echo "   sleep 5"
echo ""
echo "5. Check if summary was generated:"
echo "   ls -la .entire/summaries/"
echo "   cat .entire/summaries/*.json | jq ."
echo ""
echo "6. Expected summary structure:"
echo "   {"
echo "     \"motivation\": \"...\","
echo "     \"approach\": \"...\","
echo "     \"challenges\": \"...\","
echo "     \"tradeoffs\": \"...\","
echo "     \"outcome\": \"...\""
echo "   }"
echo ""
echo "7. Start a new Codex session and verify context:"
echo "   /Users/jqwang/01-agent/new-codex/codex-rs/target/release/codex"
echo "   Ask: 'What did we work on in the previous session?'"
echo "   The agent should have access to the summary in its context."
echo ""
echo "8. Check logs for any errors:"
echo "   tail -f ~/.codex/logs/codex.log | grep -i 'entire\|summary'"
echo ""
echo "=== Test Setup Complete ==="
