# Contributing to Surreal-Mem-MCP

First off, thank you for considering contributing! This project is built for the broader agentic developer community and every contribution matters.

---

## How to Contribute

### Reporting Bugs
- Open an issue via GitHub Issues.
- Describe the problem clearly, with reproduction steps.
- Include your OS, architecture, and the agent framework you're using.

### Feature Requests
- Open a GitHub Discussion or Issue tagged `enhancement`.
- Describe the use case and why it benefits the broader agentic ecosystem.

### Submitting Pull Requests

1. **Fork** the repository.
2. **Create a branch** from `main`: `git checkout -b feat/my-feature`
3. **Build and test** your changes: `cargo test`
4. **Ensure code compiles cleanly** with no new warnings: `cargo build`
5. **Commit** with clear, conventional messages: `feat: add X`, `fix: resolve Y`.
6. **Open a PR** against `main` with a clear description of your changes.

---

## Code Style
- This project is written in **Rust** (edition 2024). Follow standard Rust conventions.
- Run `cargo fmt` and `cargo clippy` before submitting a PR.
- Comments should explain *why*, not *what*.

## Testing
- All new features or bug fixes must include an accompanying unit test in the relevant module.
- Run the full suite with `cargo test`.
- Integration tests that touch the live SurrealDB instance should be tagged `#[ignore]` and only run manually.

---

## Development Setup

```bash
# Clone the repo
git clone https://github.com/YOUR_ORG/surreal-mem-mcp.git
cd surreal-mem-mcp

# Copy the example env file and set up a local embedding model
cp .env.example .env

# Build
cargo build

# Run tests
cargo test
```

---

## Code of Conduct
This project follows the [Contributor Covenant](https://www.contributor-covenant.org/). Please be kind and respectful to other contributors.
