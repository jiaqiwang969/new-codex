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
    ENV_CMD=\"nix develop -c bash -c\"
else
    ENV_CMD=\"bash -c\"
fi

eval \"\$ENV_CMD \\\"
    echo '========================================'
    echo '🛠️ Starting Codex in Self-Debug Mode...'
    echo '========================================'
    
    PANIC_CONTEXT=\\\"\\\"
    if [ -f \\\"last_panic.log\\\" ]; then
        PANIC_CONTEXT=\\\"\n\nThe panic backtrace is:\n\n\$(cat last_panic.log)\\\"
    fi

    echo '🔧 Checking for `entire` CLI tool...'
    if ! command -v entire &> /dev/null; then
        echo '📦 Installing `entire` for the sandbox...'
        # We install it to a local bin dir and add to PATH
        mkdir -p ~/bin
        # Assuming `entire` is either installable via cargo or we just fetch a known script/binary.
        # Since it's a rust tool, cargo install is the safest fallback if there isn't a direct binary link.
        # However, to be fast, if it's hosted on GitHub releases, we could curl it.
        # Let's use `cargo install --git https://github.com/jiaqiwang969/cli` as a robust method,
        # but wait, that takes a minute. Let's just write a wrapper script or use cargo install.
        if ! [ -x "$(command -v cargo)" ]; then
            export PATH="$HOME/.cargo/bin:$PATH"
        fi
        cargo install --git https://github.com/jiaqiwang969/cli entire --locked || true
        export PATH="$HOME/.cargo/bin:~/bin:$PATH"
    fi

    echo '⚙️ Compiling local Linux binary for debugging...'
    if [ -f \\\"Cargo.toml\\\" ]; then
        # Check if codex-cli exists to run a specific package, otherwise just run
        if grep -q \\\"codex-cli\\\" Cargo.toml; then
            cargo build -p codex-cli
            BIN_PATH=\\\"./target/debug/codex-cli\\\"
        else
            cargo build
            BIN_PATH=\\\"./target/debug/\$(basename \$PWD)\\\"
        fi
        
        echo '🚀 Booting the clone...'
        \$BIN_PATH \\\\\\\"I am a cloned instance of Codex running in an isolated VM sandbox. I just crashed or exhibited a logic bug on my macOS host. My code and runtime state (including ~/.codex Entire checkpoints) have been perfectly frozen and cloned here.\${PANIC_CONTEXT}\n\nUse \`entire rewind\` to go back to the state before the crash, analyze the project source code to find the root cause, fix it, and verify it with tests/builds. Do not use any MCP or external API, focus on my own source code.\\\\\\\"
    else
        echo '⚠️ Not a Rust project? Falling back to global codex...'
        codex \\\\\\\"I am a cloned instance of Codex running in an isolated VM sandbox. I just crashed or exhibited a logic bug on my macOS host. My code and runtime state (including ~/.codex Entire checkpoints) have been perfectly frozen and cloned here.\${PANIC_CONTEXT}\n\nUse \`entire rewind\` to go back to the state before the crash, analyze the project source code to find the root cause, fix it, and verify it with tests/builds. Do not use any MCP or external API, focus on my own source code.\\\\\\\"
    fi
\\\"\"
INNER_EOF
    chmod +x ~/start-debug.sh
"

echo "========================================"
echo "✅ Environment Frozen and Cloned successfully!"
echo "   Access your sandbox by running:"
echo "   orb -m ${VM_NAME} -u jqwang bash"
echo "   ~/start-debug.sh"
echo "========================================"
