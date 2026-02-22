#!/usr/bin/env bash
set -e

# Accept the host directory as the first argument, default to new-codex if not provided.
SOURCE_CODEX_DIR_HOST="${1:-/Users/jqwang/01-agent/new-codex}"
# Convert macOS host path to OrbStack mount path
SOURCE_CODEX_DIR="/mnt/mac${SOURCE_CODEX_DIR_HOST}"
SOURCE_DOT_CODEX="/mnt/mac${HOME}/.codex"
PROJECT_NAME=$(basename "${SOURCE_CODEX_DIR_HOST}")
VM_NAME="nixos-agent-debug-$(date +%s)"

echo "========================================"
echo "🥶 Codex Freeze & Debug Environment Cloner"
echo "========================================"
echo "Target Project: ${SOURCE_CODEX_DIR_HOST}"

echo "=> 1. Snapshotting and cloning base VM (zero-cost clone)..."
# We clone from an idle base agent to avoid disrupting nixos-dev
orb clone nixos-agent-02 ${VM_NAME} >/dev/null
orb start ${VM_NAME} >/dev/null

echo "=> 2. Injecting Frozen State into Sandbox..."
orb -m ${VM_NAME} -u jqwang bash -c "
    echo '  -> Copying source code (excluding mac target dir & locks)...'
    mkdir -p ~/${PROJECT_NAME}
    cd ${SOURCE_CODEX_DIR} && tar -cf - --exclude='target' --exclude='.git/index.lock' . | (cd ~/${PROJECT_NAME} && tar -xf -)
    
    echo '  -> Copying runtime state (~/.codex)...'
    mkdir -p ~/.codex
    cd ${SOURCE_DOT_CODEX} && tar -cf - --exclude='*.sock' --exclude='*.lock' . | (cd ~/.codex && tar -xf -)
    
    echo '  -> Creating auto-debug entrypoint script...'
    cat << 'INNER_EOF' > ~/start-debug.sh
#!/usr/bin/env bash
cd ~/${PROJECT_NAME}

# If there is a flake.nix, wrap the command in nix develop
if [ -f flake.nix ]; then
    # Inject sqlite into the nix shell alongside the flake's devShell
    ENV_CMD=\"nix develop -c nix shell nixpkgs#sqlite -c bash\"
else
    ENV_CMD=\"bash\"
fi

# We write the inner execution logic to a temporary script to avoid complex eval quoting issues
cat << 'EXEC_EOF' > /tmp/debug-exec.sh
#!/usr/bin/env bash
echo '========================================'
echo '🛠️ Starting Codex in Self-Debug Mode...'
echo '========================================'

PANIC_CONTEXT=\"\"
if [ -f \"last_panic.log\" ]; then
    PANIC_CONTEXT=\"\n\nThe panic backtrace is:\n\n\$(cat last_panic.log)\"
fi

# Ensure git identity exists in the sandbox so commits don't fail
if ! git config --global user.name >/dev/null 2>&1; then
    git config --global user.name \"jiaqiwang969\"
    git config --global user.email \"jiaqiwang969@gmail.com\"
fi

echo '🔧 Checking for \`entire\` CLI tool...'
if ! command -v entire > /dev/null 2>&1; then
    echo '📦 Installing \`entire\` for the sandbox...'
    mkdir -p ~/bin
    if ! command -v cargo > /dev/null 2>&1; then
        export PATH=\"\$HOME/.cargo/bin:\$PATH\"
    fi
    cargo install --git https://github.com/jiaqiwang969/cli entire --locked || true
    export PATH=\"\$HOME/.cargo/bin:~/bin:\$PATH\"
fi

echo '⚙️ Compiling local Linux binary for debugging...'
if [ -f \"Cargo.toml\" ]; then
    if grep -q \"codex-cli\" Cargo.toml; then
        cargo build -p codex-cli
        BIN_PATH=\"./target/debug/codex-cli\"
    else
        cargo build
        BIN_PATH=\"./target/debug/\$(basename \$PWD)\"
    fi
else
    BIN_PATH=\"codex\"
fi

echo '🚀 Booting the clone...'
cat << 'PROMPT_EOF' > /tmp/debug-prompt.txt
[META-DEBUG INSTRUCTIONS]
You are a 'Cyber-Forensic' Codex agent running in an isolated, secure NixOS VM sandbox.

The user was interacting with a *previous* instance of Codex on their macOS host, but that instance either hard-crashed (Panic) or exhibited severely buggy logic. The user triggered a `/freeze` command, which instantly paused time, cloned their entire workspace and `~/.codex` runtime database, and spawned YOU in this new thread to investigate what went wrong.

YOUR MISSION:
1. READ THE DOSSIER: You are in a *new* empty thread. To understand what the 'dead' Codex was doing before it crashed, use `entire explain <TURN_ID>` (if a Turn ID is provided below) or query `~/.codex/state_5.sqlite` directly using sqlite3 to read the chat history and prompt context.
2. CRIME SCENE: The code in this directory is EXACTLY in the dirty state it was when the bug happened. You do not need to rewind to see the bug. Start debugging the codebase immediately.
3. NO INTERACTIVE HANGS: DO NOT run `entire rewind` without specific arguments, or it will pop up an interactive menu and hang you forever. If you must revert, use `entire rewind <hash>` or `git checkout`.
4. DIAGNOSE & FIX: Use `rg`, `cat`, and `git` to find the buggy Rust logic in the Codex source code and fix it.
5. VERIFY: Run `cargo check -p codex-cli` or `cargo test` to ensure your fix compiles. DO NOT USE EXTERNAL APIS. Rely entirely on local tools and source code.
PROMPT_EOF

echo -e \"\$PANIC_CONTEXT\" >> /tmp/debug-prompt.txt

\$BIN_PATH \"\$(cat /tmp/debug-prompt.txt)\"
EXEC_EOF

chmod +x /tmp/debug-exec.sh
\$ENV_CMD /tmp/debug-exec.sh
INNER_EOF
    chmod +x ~/start-debug.sh
"

echo "========================================"
echo "✅ Environment Frozen and Cloned successfully!"
echo "   Access your sandbox by running:"
echo "   orb -m ${VM_NAME} -u jqwang bash"
echo "   ~/start-debug.sh"
echo "========================================"
