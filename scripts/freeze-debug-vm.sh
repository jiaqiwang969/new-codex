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
    echo '  -> Copying source code (excluding mac target dir)...'
    mkdir -p ~/${PROJECT_NAME}
    cd ${SOURCE_CODEX_DIR} && tar -cf - --exclude='target' . | (cd ~/${PROJECT_NAME} && tar -xf -)
    
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
    codex \\\\\\\"I am a cloned instance of Codex running in an isolated VM sandbox. I just panicked and crashed my macOS host. My code and runtime state (including ~/.codex Entire checkpoints) have been perfectly frozen and cloned here. Use \`entire rewind\` to go back to the state before the crash, analyze the project source code to find the root cause, fix it, and verify it. Do not use any MCP or external API, focus on my own source code.\\\\\\\"
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
