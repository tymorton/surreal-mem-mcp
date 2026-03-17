# Surreal Memory MCP: Architectural Benchmarks & Tradeoffs

## Executive Summary
This document outlines the performance telemetry recorded when comparing the `surreal-mem-mcp` (SurrealDB + RocksDB Edge Daemon) architecture against traditional agentic `SQLite` implementations. It is designed to assist engineers in understanding the capabilities of our autonomous, edge-deployed Server-Sent Events (SSE) server.

## Quantitative Graph Latency

The standout feature of integrating SurrealDB deeply into agentic reasoning loops is the transition from simple KV-chains to deep Knowledge Graphs. When modeling 100 continuous entity hops, we observed the following latencies:

| Backend | 1-Hop Retrieval Latency | 5-Hop Deep Traversal Latency |
|---------|-------------------------|------------------------------|
| **SQLite (Standard Native)** | ~30.6 µs | *N/A (Not Supported Natively)* |
| **SurrealDB MCP (RocksDB)** | ~880.7 µs (via Wrapper) | ~67.88 µs (Native Graph SQL) |

### Qualitative Analysis & Graph Expressivity
- **The SQLite Baseline:** Inherently constrained to 1-hop isolated join queries (`WHERE id IN = $source`). Operating 5 hops natively would require aggressive recursive CTE overhead or application-layer iterative blocking loops (N+1 queries), destroying sub-millisecond viability.
- **The Surreal-Mem-MCP Advantage:** `surreal-mem-mcp` handles deeply nested contexts directly inside the engine via `SELECT ->relation..5->entity FROM $root`. This solves massive interconnected memory meshes in roughly **~68 µs**, unblocking autonomous RAG agents from relying exclusively on high-latency Embedding distance math.

## Hardware Resource Profiling

Testing environments were strictly isolated via Sandboxed Docker Containers (16 vCPUs).

### Memory (RAM) Overhead Comparison

| Architecture | Peak RAM Util | Avg RAM Util | Relative Size |
|:---|:---|:---|:---|
| **Simple SQLite Embed** | 62.6 MB | 58.0 MB | `1.0x` |
| **Surreal-Mem-MCP API Server** | 163.5 MB | 155.2 MB | `2.6x` |

**Takeaway:** Operating an HTTP/SSE server alongside an embedded Rust Multi-Model indexing engine commands a higher RAM footprint relative to simple C-binding library integrations. The ~155MB average footprint remains incredibly lightweight for edge-inference but is a documented compute factor.

### Storage & Vector Density (Disk IO)

| Architecture | Storage Topology | Size (1000 Memories) | Density Efficiency |
|:---|:---|:---|:---|
| **SQLite Baseline** | `.db` B-Tree File | 0.41 MB | `1.0x` |
| **Surreal-Mem-MCP** | RocksDB SST Block Log | 0.34 MB | `0.82x` |

**Takeaway:** RocksDB significantly compresses dense memory trees and string structures. The memory hierarchy actively packs the filesystem ~18% more efficiently over sustained usage durations without requiring manual `VACUUM` locks.
