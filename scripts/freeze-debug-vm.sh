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
    ENV_CMD=\"nix develop -c nix shell nixpkgs#sqlite -c bash -c\"
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

    # Ensure git identity exists in the sandbox so commits don't fail
    if ! git config --global user.name >/dev/null 2>&1; then
        git config --global user.name "jiaqiwang969"
        git config --global user.email "jiaqiwang969@gmail.com"
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
        \$BIN_PATH \\\\\\\"[META-DEBUG INSTRUCTIONS]\nYou are a 'Cyber-Forensic' Codex agent running in an isolated, secure NixOS VM sandbox.\n\nThe user was interacting with a *previous* instance of Codex on their macOS host, but that instance either hard-crashed (Panic) or exhibited severely buggy logic. The user triggered a `/freeze` command, which instantly paused time, cloned their entire workspace and `~/.codex` runtime database, and spawned YOU in this new thread to investigate what went wrong.\n\${PANIC_CONTEXT}\n\nYOUR MISSION:\n1. READ THE DOSSIER: You are in a *new* empty thread. To understand what the 'dead' Codex was doing before it crashed, use `entire explain <TURN_ID>` (if a Turn ID is provided above) or query `~/.codex/state_5.sqlite` directly using sqlite3 to read the chat history and prompt context.\n2. CRIME SCENE: The code in this directory is EXACTLY in the dirty state it was when the bug happened. You do not need to rewind to see the bug. Start debugging the codebase immediately.\n3. NO INTERACTIVE HANGS: DO NOT run `entire rewind` without specific arguments, or it will pop up an interactive menu and hang you forever. If you must revert, use `entire rewind <hash>` or `git checkout`.\n4. DIAGNOSE & FIX: Use `rg`, `cat`, and `git` to find the buggy Rust logic in the Codex source code and fix it.\n5. VERIFY: Run `cargo check -p codex-cli` or `cargo test` to ensure your fix compiles. DO NOT USE EXTERNAL APIS. Rely entirely on local tools and source code.\\\\\\\"
    else
        echo '⚠️ Not a Rust project? Falling back to global codex...'
        codex \\\\\\\"[META-DEBUG INSTRUCTIONS]\nYou are a 'Cyber-Forensic' Codex agent running in an isolated, secure NixOS VM sandbox.\n\nThe user was interacting with a *previous* instance of Codex on their macOS host, but that instance either hard-crashed (Panic) or exhibited severely buggy logic. The user triggered a `/freeze` command, which instantly paused time, cloned their entire workspace and `~/.codex` runtime database, and spawned YOU in this new thread to investigate what went wrong.\n\${PANIC_CONTEXT}\n\nYOUR MISSION:\n1. READ THE DOSSIER: You are in a *new* empty thread. To understand what the 'dead' Codex was doing before it crashed, use `entire explain <TURN_ID>` (if a Turn ID is provided above) or query `~/.codex/state_5.sqlite` directly using sqlite3 to read the chat history and prompt context.\n2. CRIME SCENE: The code in this directory is EXACTLY in the dirty state it was when the bug happened. You do not need to rewind to see the bug. Start debugging the codebase immediately.\n3. NO INTERACTIVE HANGS: DO NOT run `entire rewind` without specific arguments, or it will pop up an interactive menu and hang you forever. If you must revert, use `entire rewind <hash>` or `git checkout`.\n4. DIAGNOSE & FIX: Use `rg`, `cat`, and `git` to find the buggy Rust logic in the Codex source code and fix it.\n5. VERIFY: Run `cargo check -p codex-cli` or `cargo test` to ensure your fix compiles. DO NOT USE EXTERNAL APIS. Rely entirely on local tools and source code.\\\\\\\"
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
