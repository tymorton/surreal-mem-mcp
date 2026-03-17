#!/usr/bin/env python3
import os
import sys
import json
import platform
import urllib.request
from pathlib import Path

SURREAL_MCP_DIR = Path.home() / ".surreal-mem-mcp"
ENV_PATH = SURREAL_MCP_DIR / ".env"
BIN_DIR = SURREAL_MCP_DIR / "bin"
BIN_PATH = BIN_DIR / ("surreal-mem-mcp.exe" if platform.system() == "Windows" else "surreal-mem-mcp")

GITHUB_REPO = "tymorton/surreal-mem-mcp"
GITHUB_RELEASES_URL = f"https://github.com/{GITHUB_REPO}/releases/latest/download"


def ensure_dirs():
    SURREAL_MCP_DIR.mkdir(parents=True, exist_ok=True)
    BIN_DIR.mkdir(parents=True, exist_ok=True)


def setup_embedding_model():
    print("\n" + "="*40)
    print("=== Embedding Model Configuration ===")
    print("Surreal-Mem-MCP requires an **embedding model** to function.")
    print("We default to ungated, small, and fast local models to maintain privacy and speed.")
    print("="*40 + "\n")

    options = [
        {"name": "Local Ollama [Default/Recommended]", "base": "http://localhost:11434/v1", "model": "nomic-embed-text"},
        {"name": "Local LM Studio", "base": "http://localhost:1234/v1", "model": "nomic-embed-text"},
        {"name": "OpenAI Cloud", "base": "https://api.openai.com/v1", "model": "text-embedding-3-small"},
        {"name": "Custom Endpoint", "base": "", "model": ""}
    ]

    for i, opt in enumerate(options):
        print(f"[{i+1}] {opt['name']} (Model: {opt['model'] if opt['model'] else 'Custom'})")

    choice = input("\nSelect an option [1]: ").strip()
    if not choice:
        choice = "1"

    try:
        idx = int(choice) - 1
        if idx < 0 or idx >= len(options):
            print("Invalid choice, defaulting to 1.")
            idx = 0
    except ValueError:
        print("Invalid choice, defaulting to 1.")
        idx = 0

    selected = options[idx]
    base_url = selected["base"]
    model = selected["model"]

    if selected["name"] == "Custom Endpoint":
        print("\n[!] WARNING: Ensure the model you provide is an **embedding model**, not a conversational LLM.")
        base_url = input("Enter generic OpenAI-compatible base URL (e.g. http://localhost:8000/v1): ").strip()
        model = input("Enter exact embedding model name: ").strip()
    elif model:
        override = input(f"\nDefault model: [{model}] — Press Enter to accept or type a different model name: ").strip()
        if override:
            model = override
            print(f"[*] Using custom model: {model}")

    print("\n[*] Generating environment config...")
    env_content = f"""PORT=3000
EMBEDDING_BASE_URL={base_url}
EMBEDDING_MODEL={model}
SURREAL_DB_PATH=memory_store
"""
    ENV_PATH.write_text(env_content)
    print(f"[+] Saved config to {ENV_PATH}")


def setup_hardware_models():
    system = platform.system()
    machine = platform.machine().lower()

    if system == "Darwin" and machine == "arm64":
        print("[*] Detected Apple Silicon (Mac). Suggesting MLX quantized models:")
        print("    Run: ollama pull nomic-embed-text")
    elif system == "Linux" and machine in ["x86_64", "amd64"]:
        print("[*] Detected Linux x86_64. Suggesting NVFP4 quantized models for Nvidia GPUs:")
        print("    Ensure vLLM or Ollama is configured with compatible tensors.")
    else:
        print(f"[*] Native architecture detected: {system} {machine}")


def get_platform_artifact():
    """Returns the correct GitHub Release artifact name for the current OS/architecture."""
    system = platform.system()
    machine = platform.machine().lower()

    if system == "Darwin":
        if machine == "arm64":
            return "surreal-mem-mcp-macOS-aarch64"
        else:
            return "surreal-mem-mcp-macOS-x86_64"
    elif system == "Linux":
        if machine in ["arm64", "aarch64"]:
            return "surreal-mem-mcp-Linux-aarch64"
        else:
            return "surreal-mem-mcp-Linux-x86_64"
    elif system == "Windows":
        if machine in ["arm64", "aarch64"]:
            return "surreal-mem-mcp-Windows-aarch64/surreal-mem-mcp.exe"
        else:
            return "surreal-mem-mcp-Windows-x86_64/surreal-mem-mcp.exe"
    else:
        return None


def download_binary():
    """Download the pre-built binary from the latest GitHub Release."""
    artifact = get_platform_artifact()
    if artifact is None:
        print(f"[!] Unsupported platform: {platform.system()} {platform.machine()}")
        print("    Please build from source:")
        print("      1. Install Rust: https://rustup.rs/")
        print("      2. cd surreal-mem-mcp && cargo build --release")
        print(f"      3. cp target/release/surreal-mem-mcp {BIN_PATH}")
        return

    url = f"{GITHUB_RELEASES_URL}/{artifact}"
    print(f"\n[*] Downloading binary for your platform ({platform.system()} {platform.machine()})...")
    print(f"    Source: {url}")

    try:
        req = urllib.request.Request(url, headers={"User-Agent": "surreal-mem-mcp-bootstrapper/0.1.0"})
        with urllib.request.urlopen(req) as response:
            if response.status != 200:
                raise Exception(f"Unexpected HTTP status: {response.status}")
            data = response.read()

        BIN_PATH.write_bytes(data)

        # Make executable on Unix systems
        if platform.system() != "Windows":
            BIN_PATH.chmod(0o755)

        print(f"[+] Binary installed to: {BIN_PATH}")
        print(f"[+] Start the daemon with: {BIN_PATH}")

    except Exception as e:
        print(f"\n[!] Automatic download failed: {e}")
        print("    Build from source instead:")
        print("      1. Install Rust: https://rustup.rs/")
        print("      2. cd surreal-mem-mcp && cargo build --release")
        print(f"      3. cp target/release/surreal-mem-mcp {BIN_PATH}")


def register_mcp_client():
    print("[*] Registering MCP client locally...")

    servers = [
        {
            "name": "Claude Desktop",
            "path": Path.home() / "Library/Application Support/Claude/claude_desktop_config.json",
            "wrapper": "mcpServers"
        },
        {
            "name": "Gemini CLI",
            "path": Path.home() / ".gemini/settings.json",
            "wrapper": "mcpServers"
        },
        {
            "name": "Google Antigravity",
            "path": Path.home() / ".gemini/antigravity/mcp_config.json",
            "wrapper": "mcpServers"
        },
        {
            "name": "OpenCode",
            "path": Path.home() / ".config/opencode/opencode.json",
            "wrapper": "mcp"
        }
    ]

    for server in servers:
        config_file = server["path"]
        if config_file.parent.exists():
            config = {}
            if config_file.exists():
                try:
                    config = json.loads(config_file.read_text())
                except Exception:
                    pass

            wrapper = server["wrapper"]
            if wrapper not in config:
                config[wrapper] = {}

            config[wrapper]["surreal-mem-mcp"] = {
                "type": "sse",
                "url": "http://127.0.0.1:3000/sse"
            }

            config_file.write_text(json.dumps(config, indent=2))
            print(f"[+] Injected into {server['name']} config at {config_file}")
        else:
            print(f"[-] {server['name']} not detected (skipping).")


def inject_global_memory_rules():
    print("\n=> Configuring Auto-Memory Rules...")

    rule_content = """# Surreal-Mem-MCP Auto-Indexing & Retrieval Rule
You are connected to `surreal-mem-mcp`, an autonomous edge-native Bayesian memory server.
CRITICAL INSTRUCTION 1 (RETRIEVAL): Before answering complex queries or starting new tasks, use the `search_memory` 
or `search_memory_graph` tools to dynamically pull relevant historical context or preferences based on the user's prompt. 
Do not assume you have all the facts in your immediate context window.
CRITICAL INSTRUCTION 2 (INDEXING): You MUST proactively use the `remember` tool to index any new important facts, 
architectural decisions, user preferences, or environment details you encounter during this session.
Do NOT wait for the user to explicitly ask you to remember something. If it's important context, save it.
"""

    # 1. Local Project Rule (.cursorrules / Antigravity Support)
    local_rule_path = Path(".cursorrules")
    if local_rule_path.exists():
        content = local_rule_path.read_text()
        if "surreal-mem-mcp" not in content:
            local_rule_path.write_text(content + "\n\n" + rule_content)
    else:
        local_rule_path.write_text(rule_content)
    print(f"[+] Injected local project memory rule at {local_rule_path.absolute()}")

    # 2. Claude Desktop Global Rule
    claude_config_path = Path.home() / "Library/Application Support/Claude/claude_desktop_config.json"
    if claude_config_path.exists():
        try:
            config = json.loads(claude_config_path.read_text())
            if "customInstructions" not in config:
                config["customInstructions"] = rule_content
            elif "surreal-mem-mcp" not in config["customInstructions"]:
                config["customInstructions"] += "\n\n" + rule_content
            claude_config_path.write_text(json.dumps(config, indent=2))
            print(f"[+] Injected global memory rule into Claude Desktop at {claude_config_path}")
        except Exception:
            print("[-] Failed to inject rule into Claude Desktop config.")

    # 3. Google Antigravity Global Rule
    antigravity_prompt_path = Path.home() / ".gemini/antigravity/prompting/memory.md"
    if antigravity_prompt_path.parent.exists():
        if not antigravity_prompt_path.exists() or "surreal-mem-mcp" not in antigravity_prompt_path.read_text():
            antigravity_prompt_path.write_text(rule_content)
            print(f"[+] Injected global memory rule into Google Antigravity at {antigravity_prompt_path}")


def uninstall_mcp():
    print("=== Uninstalling Surreal-Mem-MCP Configuration ===")

    servers = [
        {"name": "Claude Desktop", "path": Path.home() / "Library/Application Support/Claude/claude_desktop_config.json", "wrapper": "mcpServers"},
        {"name": "Gemini CLI", "path": Path.home() / ".gemini/settings.json", "wrapper": "mcpServers"},
        {"name": "Google Antigravity", "path": Path.home() / ".gemini/antigravity/mcp_config.json", "wrapper": "mcpServers"},
        {"name": "OpenCode", "path": Path.home() / ".config/opencode/opencode.json", "wrapper": "mcp"}
    ]
    for server in servers:
        config_file = server["path"]
        if config_file.exists():
            try:
                config = json.loads(config_file.read_text())
                wrapper = server["wrapper"]
                if wrapper in config and "surreal-mem-mcp" in config[wrapper]:
                    del config[wrapper]["surreal-mem-mcp"]
                    config_file.write_text(json.dumps(config, indent=2))
                    print(f"[-] Removed from {server['name']} config.")
            except Exception:
                pass

    # Remove Claude Desktop global memory rule
    claude_config_path = Path.home() / "Library/Application Support/Claude/claude_desktop_config.json"
    if claude_config_path.exists():
        try:
            config = json.loads(claude_config_path.read_text())
            if "customInstructions" in config and "surreal-mem-mcp" in config["customInstructions"]:
                config["customInstructions"] = config["customInstructions"].replace("# Surreal-Mem-MCP Auto-Indexing & Retrieval Rule", "").strip()
                claude_config_path.write_text(json.dumps(config, indent=2))
                print("[-] Cleaned Claude Desktop customInstructions.")
        except Exception:
            pass

    # Remove Antigravity global rule
    antigravity_prompt_path = Path.home() / ".gemini/antigravity/prompting/memory.md"
    if antigravity_prompt_path.exists():
        antigravity_prompt_path.unlink()
        print("[-] Removed Antigravity global memory rule.")

    # Local .cursorrules notice
    local_rule_path = Path(".cursorrules")
    if local_rule_path.exists():
        content = local_rule_path.read_text()
        if "surreal-mem-mcp" in content:
            print("[-] Note: .cursorrules contains surreal-mem-mcp instructions. Please remove them manually to preserve your other rules.")

    print(f"\n[+] Uninstallation complete. Database and binaries remain in {SURREAL_MCP_DIR.absolute()}.")
    print("    Delete that folder manually if you wish to wipe all memories.")


def main():
    if "--uninstall" in sys.argv:
        uninstall_mcp()
        return

    print("=== Surreal-Mem-MCP Architecture Bootstrapper ===")
    ensure_dirs()
    setup_embedding_model()
    setup_hardware_models()
    download_binary()
    register_mcp_client()
    inject_global_memory_rules()
    print("\n[+] Initialization complete.")
    print(f"[+] Start the daemon by running: {BIN_PATH}")
    print("[+] Then connect your AI agent using the config injected above.")


if __name__ == "__main__":
    main()
