#!/usr/bin/env bash
set -e

SOURCE_CODEX_DIR_HOST="${1:-/Users/jqwang/01-agent/new-codex}"
SOURCE_CODEX_DIR="/mnt/mac${SOURCE_CODEX_DIR_HOST}"
VM_NAME="nixos-agent-debug-$(date +%s)"

echo "========================================"
echo "🥶 Codex Freeze & Debug Environment Spawner"
echo "========================================"
echo "Target Project: ${SOURCE_CODEX_DIR_HOST}"

echo "=> 1. Spinning up debug VM (fast clone)..."
orb clone nixos-dev ${VM_NAME} >/dev/null
orb start ${VM_NAME} >/dev/null

echo "=> 2. Configuring Debug Sandbox..."
orb -m ${VM_NAME} -u jqwang bash << USER_EOF
echo '  -> Creating auto-debug entrypoint script...'
cat << 'INNER_EOF' > ~/start-debug.sh
#!/usr/bin/env bash
cd ${SOURCE_CODEX_DIR}

# Auto-cleanup on exit
trap 'echo ""; read -p "🗑️  Debug session ended. Delete ephemeral VM (${VM_NAME})? [Y/n] " -n 1 -r; echo; if [[ \$$REPLY =~ ^[Yy]\$$ ]] || [[ -z \$$REPLY ]]; then orb delete ${VM_NAME}; fi' EXIT

if [ -f flake.nix ]; then
    ENV_CMD="nix develop -c nix shell nixpkgs#sqlite -c bash"
else
    ENV_CMD="bash"
fi

cat << 'EXEC_EOF' > /tmp/debug-exec.sh
#!/usr/bin/env bash
echo '========================================'
echo '🛠️ Starting Codex in Self-Debug Mode...'
echo '========================================'

PANIC_CONTEXT=""
if [ -f "last_panic.log" ]; then
    PANIC_CONTEXT="

The panic backtrace is:

\$(cat last_panic.log)"
fi

echo '🔧 Checking for \`entire\` CLI tool...'
if ! command -v entire > /dev/null 2>&1; then
    echo '📦 Installing \`entire\` for the sandbox...'
    mkdir -p ~/bin
    if ! command -v cargo > /dev/null 2>&1; then
        export PATH="\$HOME/.cargo/bin:\$PATH"
    fi
    cargo install --git https://github.com/jiaqiwang969/cli entire --locked || true
    export PATH="\$HOME/.cargo/bin:~/bin:\$PATH"
fi

echo '⚙️ Compiling local Linux binary for debugging...'
if [ -f "Cargo.toml" ]; then
    if grep -q "codex-cli" Cargo.toml; then
        cargo build -p codex-cli
        BIN_PATH="./target/debug/codex-cli"
    else
        cargo build
        BIN_PATH="./target/debug/\$(basename \$PWD)"
    fi
else
    BIN_PATH="codex"
fi

echo '🚀 Booting the clone...'
cat << 'PROMPT_EOF' > /tmp/debug-prompt.txt
[META-DEBUG INSTRUCTIONS]
You are a 'Cyber-Forensic' Codex agent running in a NixOS VM sandbox.

The user triggered a \`/freeze\` command, spawning YOU in this new thread to investigate what went wrong. You have direct access to the live code via OrbStack virtiofs mounts.

YOUR MISSION:
1. READ THE DOSSIER: You are in a *new* empty thread. To understand what the 'dead' Codex was doing before it crashed, use \`entire explain <TURN_ID>\` (if a Turn ID is provided below) or query \`~/.codex/state_5.sqlite\` directly using sqlite3 to read the chat history and prompt context.
2. CRIME SCENE: Start debugging the codebase immediately.
3. NO INTERACTIVE HANGS: DO NOT run \`entire rewind\` without specific arguments, or it will pop up an interactive menu and hang you forever. If you must revert, use \`entire rewind <hash>\` or \`git checkout\`.
4. DIAGNOSE & FIX: Use \`rg\`, \`cat\`, and \`git\` to find the buggy Rust logic in the Codex source code and fix it.
5. VERIFY: Run \`cargo check -p codex-cli\` or \`cargo test\` to ensure your fix compiles. DO NOT USE EXTERNAL APIS. Rely entirely on local tools and source code.
PROMPT_EOF

echo -e "\$PANIC_CONTEXT" >> /tmp/debug-prompt.txt

\$BIN_PATH "\$(cat /tmp/debug-prompt.txt)"
EXEC_EOF

chmod +x /tmp/debug-exec.sh
\$ENV_CMD /tmp/debug-exec.sh
INNER_EOF
chmod +x ~/start-debug.sh
USER_EOF

echo "========================================"
echo "✅ Environment Frozen and Cloned successfully!"
echo "   Opening Terminal for debugging... (VM: ${VM_NAME})"
echo "========================================"

# Launch macOS Terminal natively to attach
osascript -e 'tell application "Terminal"' -e 'activate' -e "do script \"orb -m ${VM_NAME} -u jqwang bash -c '~/start-debug.sh'\"" -e 'end tell'
