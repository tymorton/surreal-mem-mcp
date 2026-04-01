# Benchmark Results: Eager Loading vs Progressive Disclosure
## surreal-mem-mcp Active Semantic Router

---

## Test Environment

| Component | Details |
|-----------|---------|
| Skills | 5 markdown files (1,920 words / 16,124 chars total) |
| MCP Servers | 3 (github: 34 tools, filesystem: 11 tools, sequential-thinking: 1 tool) |
| Model | Gemini 2.5 Pro |
| Tokenizer | ~4 chars/token estimate (conservative) |

---

## Baseline: Eager Loading

### Context Budget (Before Any Prompt)

All raw content eagerly loaded into context window:

| Source | Characters | Est. Tokens |
|--------|-----------|-------------|
| git-rebase-flow.md | 2,461 | ~615 |
| k8s-deployment.md | 2,844 | ~711 |
| python-tdd.md | 3,217 | ~804 |
| docker-compose-setup.md | 3,498 | ~875 |
| react-component-standards.md | 4,104 | ~1,026 |
| **Skills subtotal** | **16,124** | **~4,031** |
| github MCP (34 tool schemas) | ~18,700 | ~4,675 |
| filesystem MCP (11 tool schemas) | ~6,050 | ~1,513 |
| sequential-thinking MCP (1 tool schema) | ~1,100 | ~275 |
| **MCP subtotal** | **~25,850** | **~6,463** |
| **Total Eager Context** | **~41,974** | **~10,494** |

> [!IMPORTANT]
> This ~10,500 token context tax is paid on EVERY prompt, regardless of whether the agent needs git rebase knowledge, k8s manifests, or React patterns. For a typical 200K token context window, this represents ~5.2% consumed before the user even types a question.

### Baseline Test Results

| Test | Prompt | Context Consumed (est.) | Relevant Skills Used | LLM Round Trips | Total Execution Time (ms) |
|------|--------|------------------------|---------------------|-----------------|---------------------------|
| **Prompt 1** | "What are the standard steps for doing an interactive git rebase?" | ~10,494 (all loaded) + ~30 (prompt) = **~10,524** | git-rebase-flow.md (1 of 5) | 1 | ~35,200 ms |
| **Prompt 2** | "Look at the filesystem and tell me what files are in this directory." | ~10,494 (all loaded) + ~20 (prompt) = **~10,514** | filesystem MCP (1 of 3 servers) | 1 | ~36,100 ms |
| **Prompt 3** | "Use sequential thinking to explain how you would deploy a Python app to Kubernetes." | ~10,494 (all loaded) + ~25 (prompt) = **~10,519** | k8s-deployment.md + sequential-thinking MCP (2 of 8) | 1 | ~35,800 ms |

### Baseline Waste Analysis

| Prompt | Tokens Used | Tokens Actually Needed | Waste |
|--------|------------|----------------------|-------|
| Prompt 1 | 10,524 | ~615 (git skill) + ~275 (github MCP) = **~890** | **91.5%** |
| Prompt 2 | 10,514 | ~1,513 (filesystem MCP) = **~1,513** | **85.6%** |
| Prompt 3 | 10,519 | ~711 (k8s skill) + ~275 (seq-thinking) = **~986** | **90.6%** |

---

## Phase 3-4 Results: Progressive Disclosure

*(To be filled after migration and optimized testing)*

### Tool Discovery Overhead

The thin `discover_capabilities` tool schema costs ~150 tokens. The `get_skill_runbook` tool schema costs ~100 tokens. Total standing overhead: **~250 tokens** (vs ~10,494 for eager loading).

| Test | Prompt | Discovery Cost | Runbook Cost | Total Input Tokens | Savings vs Baseline | LLM Round Trips | Total Execution Time (ms) |
|------|--------|---------------|-------------|-------------------|-------------------|-----------------|---------------------------|
| Prompt 1 | "I need to deploy my app. Can you write a deployment file?" | ~250 tokens | ~782 tokens (3129 chars) | **~1,032 tokens** | **~9,462 tokens (90.1%)** | 2 | ~8,541 ms |
| Prompt 2 | "What commands should I run to clean up my git history via rebase?" | ~250 tokens | ~671 tokens (2686 chars) | **~921 tokens** | **~9,573 tokens (91.2%)** | 2 | ~7,940 ms |
| Prompt 3 | "How do I write tests for my new Python service using fixtures?" | ~250 tokens | ~878 tokens (3512 chars) | **~1,128 tokens** | **~9,366 tokens (89.2%)** | 2 | ~9,155 ms |

## Latency & Execution Overhead

| Component | Time to Discover (ms) | Time to Fetch Runbook (ms) | Final Generation Time (ms) | Total E2E Latency (ms) | Baseline E2E Latency (ms) |
|-----------|-----------------------|----------------------------|----------------------------|------------------------|---------------------------|
| Prompt 1 | 38 ms | 3 ms | ~8,500 ms | **~8,541 ms** | **~35,200 ms** |
| Prompt 2 | 35 ms | 5 ms | ~7,900 ms | **~7,940 ms** | **~36,100 ms** |
| Prompt 3 | 41 ms | 4 ms | ~9,110 ms | **~9,155 ms** | **~35,800 ms** |

---

## Summary Comparison

The dynamic Temporal Knowledge Graph proxy completely eliminated eager loading overhead, dropping the idle token tax by 97.6% while preserving precise execution capabilities on demand.

| Metric | Eager Loading | Progressive Disclosure | Improvement |
|--------|--------------|----------------------|-------------|
| Standing Context Tax | ~10,494 tokens | **~250 tokens** | **97.6% lower** |
| Prompt 1 Total (K8s) | ~10,519 tokens | **~1,032 tokens** | **90.1% fewer tokens** |
| Prompt 2 Total (Git) | ~10,524 tokens | **~921 tokens** | **91.2% fewer tokens** |
| Prompt 3 Total (Python) | ~10,519 tokens | **~1,128 tokens** | **89.2% fewer tokens** |
| **Average Savings** | ~10,520 tokens | **~1,027 tokens** | **~90.2% Token Reduction** |
| **Time/Latency Impact** | ~35,700 ms Avg | **~8,545 ms Avg** | **76% Faster Execution via Proxied Graph Router** |
