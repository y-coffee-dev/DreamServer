# Agent Systems Blueprint

**A complete, vendor-neutral blueprint for building a production agentic coding tool from scratch.**

32 documents. 14,384 lines. Zero proprietary code. Every subsystem documented — from security foundations to the execution engine to local LLM deployment.

> **Start here:** [AGENT-ARCHITECTURE-OVERVIEW.md](AGENT-ARCHITECTURE-OVERVIEW.md) — the master map with dependency graphs, error boundaries, and end-to-end walkthroughs.
>
> **For local AI:** [AGENT-LOCAL-LLM-ADAPTATION.md](AGENT-LOCAL-LLM-ADAPTATION.md) — bridges all cloud patterns to DreamServer's local stack.

---

## Reading Order

Build from the bottom up. Each layer depends on layers below it.

| Layer | # | Document | What It Covers |
|-------|---|----------|---------------|
| **1. Security** | 1 | [AGENT-SECURITY-COMMAND-EXECUTION.md](AGENT-SECURITY-COMMAND-EXECUTION.md) | Multi-layer shell injection prevention, AST parsing, path validation |
| | 2 | [AGENT-SECURITY-NETWORK-AND-INJECTION.md](AGENT-SECURITY-NETWORK-AND-INJECTION.md) | SSRF protection, DNS rebinding, Unicode injection defense |
| **2. Architecture** | 3 | [AGENT-PERMISSION-SYSTEM-DESIGN.md](AGENT-PERMISSION-SYSTEM-DESIGN.md) | Declarative rule-based permissions, modes, denial tracking |
| | 4 | [AGENT-TOOL-ARCHITECTURE.md](AGENT-TOOL-ARCHITECTURE.md) | Unified tool interface, MCP protocol, plugins, skills system |
| | 5 | [AGENT-COORDINATION-PATTERNS.md](AGENT-COORDINATION-PATTERNS.md) | Coordinator/worker orchestration, teammates, parallelism |
| | 6 | [AGENT-ERROR-HANDLING-AND-HOOKS.md](AGENT-ERROR-HANDLING-AND-HOOKS.md) | Error classification, event-driven hooks, HTTP hook security |
| **3. Core** | 7 | [AGENT-SYSTEM-PROMPT-ENGINEERING.md](AGENT-SYSTEM-PROMPT-ENGINEERING.md) | Section-based prompts, caching, injection defense, versioning |
| | 8 | [AGENT-CONTEXT-AND-CONVERSATION.md](AGENT-CONTEXT-AND-CONVERSATION.md) | Token budgeting, history management, compaction triggers |
| | 9 | [AGENT-LLM-API-INTEGRATION.md](AGENT-LLM-API-INTEGRATION.md) | Streaming, retry, model selection, rate limits, cost tracking |
| | 10 | [AGENT-BOOTSTRAP-AND-CONFIGURATION.md](AGENT-BOOTSTRAP-AND-CONFIGURATION.md) | Startup sequence, multi-source config, enterprise polling, migrations |
| | 11 | [AGENT-AUTH-AND-SESSION-MANAGEMENT.md](AGENT-AUTH-AND-SESSION-MANAGEMENT.md) | OAuth/PKCE, token refresh, keychain, session persistence, crash recovery |
| | 12 | [AGENT-SPECULATION-AND-CACHING.md](AGENT-SPECULATION-AND-CACHING.md) | Optimistic execution, file state overlays, stale-while-refresh |
| **4. Rendering** | 13 | [AGENT-TERMINAL-UI-ARCHITECTURE.md](AGENT-TERMINAL-UI-ARCHITECTURE.md) | React reconciler for terminals, double buffering, keyboard, mouse |
| | 14 | [AGENT-DIFF-AND-FILE-EDITING.md](AGENT-DIFF-AND-FILE-EDITING.md) | Patch generation, encoding, notebooks, change attribution |
| | 15 | [AGENT-IDE-AND-LSP-INTEGRATION.md](AGENT-IDE-AND-LSP-INTEGRATION.md) | Language Server Protocol, passive diagnostics, crash recovery |
| **5. Operations** | 16 | [AGENT-WORKTREE-AND-ISOLATION.md](AGENT-WORKTREE-AND-ISOLATION.md) | Git worktrees for parallel agents, symlinks, sparse checkout |
| | 17 | [AGENT-FEATURE-DELIVERY.md](AGENT-FEATURE-DELIVERY.md) | Auto-update, kill switch, subscription tiers, contributor safety |
| **6. Product** | 18 | [AGENT-MEMORY-AND-CONSOLIDATION.md](AGENT-MEMORY-AND-CONSOLIDATION.md) | Persistent memory, 4 types, auto-dream consolidation, team sync |
| | 19 | [AGENT-CONTEXT-COMPACTION-ADVANCED.md](AGENT-CONTEXT-COMPACTION-ADVANCED.md) | Microcompact, session compact, full compact, reactive recovery |
| | 20 | [AGENT-TASK-AND-BACKGROUND-EXECUTION.md](AGENT-TASK-AND-BACKGROUND-EXECUTION.md) | Forked agent pattern, 7 task types, cache-safe params |
| | 21 | [AGENT-REMOTE-AND-TEAM-COLLABORATION.md](AGENT-REMOTE-AND-TEAM-COLLABORATION.md) | WebSocket sessions, permission routing, teammates, teleportation |
| | 22 | [AGENT-ENTERPRISE-AND-POLICY.md](AGENT-ENTERPRISE-AND-POLICY.md) | Managed settings, policy limits, fail-open/closed, settings sync |
| | 23 | [AGENT-MESSAGE-PIPELINE.md](AGENT-MESSAGE-PIPELINE.md) | Message types, command queue, priority scheduling, collapsing |
| | 24 | [AGENT-MEDIA-AND-ATTACHMENTS.md](AGENT-MEDIA-AND-ATTACHMENTS.md) | Images, PDFs, clipboard, notebooks, ANSI rendering |
| | 25 | [AGENT-LIFECYCLE-AND-PROCESS.md](AGENT-LIFECYCLE-AND-PROCESS.md) | Graceful shutdown, cleanup, crash recovery, concurrent sessions |
| **7. Engine** | 26 | [AGENT-QUERY-LOOP-AND-STATE-MACHINE.md](AGENT-QUERY-LOOP-AND-STATE-MACHINE.md) | The main loop — 11 recovery transitions, 9 terminal conditions |
| | 27 | [AGENT-STREAMING-TOOL-EXECUTION.md](AGENT-STREAMING-TOOL-EXECUTION.md) | Concurrent tool execution, batching, size management |
| | 28 | [AGENT-SDK-BRIDGE.md](AGENT-SDK-BRIDGE.md) | Message translation, NDJSON protocol, permission routing |
| | 29 | [AGENT-INITIALIZATION-AND-WIRING.md](AGENT-INITIALIZATION-AND-WIRING.md) | 6-stage bootstrap, preflight, fast mode, prefetch ordering |
| **Meta** | 30 | [AGENT-ARCHITECTURE-OVERVIEW.md](AGENT-ARCHITECTURE-OVERVIEW.md) | **Master map** — dependency graph, error boundaries, walkthroughs |
| | 31 | [AGENT-BUILD-AND-DEPENDENCIES.md](AGENT-BUILD-AND-DEPENDENCIES.md) | Technology stack, project structure, packaging, test pyramid |
| **Mission** | 32 | [AGENT-LOCAL-LLM-ADAPTATION.md](AGENT-LOCAL-LLM-ADAPTATION.md) | **DreamServer bridge** — GPU budgeting, tool calling tiers, small models |

---

## Origin

Extracted as open-source best practices from exhaustive analysis of production agentic systems on March 31, 2026. All patterns described in original writing — zero proprietary code reproduced, zero vendor-specific terms.
