#!/usr/bin/env bash
set -e

VM_NAME="nixos-agent-debug-$(date +%s)"
SOURCE_CODEX_DIR="/mnt/mac/Users/jqwang/01-agent/new-codex"
SOURCE_DOT_CODEX="/mnt/mac/Users/jqwang/.codex"

echo "========================================"
echo "🥶 Codex Freeze & Debug Environment Cloner"
echo "========================================"

echo "=> 1. Snapshotting and cloning base VM (zero-cost clone)..."
# We clone from an idle base agent to avoid disrupting nixos-dev
orb clone nixos-agent-02 ${VM_NAME} >/dev/null
orb start ${VM_NAME} >/dev/null

echo "=> 2. Injecting Frozen State into Sandbox..."
orb -m ${VM_NAME} -u jqwang bash -c "
    echo '  -> Copying source code (excluding mac target dir)...'
    mkdir -p ~/new-codex
    cd ${SOURCE_CODEX_DIR} && tar -cf - --exclude='target' . | (cd ~/new-codex && tar -xf -)
    
    echo '  -> Copying runtime state (~/.codex)...'
    mkdir -p ~/.codex
    cd ${SOURCE_DOT_CODEX} && tar -cf - --exclude='*.sock' --exclude='*.lock' . | (cd ~/.codex && tar -xf -)
"

echo "========================================"
echo "✅ Environment Frozen and Cloned successfully!"
echo "   Access your sandbox by running:"
echo "   orb -m ${VM_NAME} -u jqwang bash"
echo "   cd ~/new-codex && nix develop"
echo "========================================"
