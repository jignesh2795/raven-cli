# Raven OSS CLI Research & Local Source Library Roadmap

**Status:** Proposed independent research and enablement track  
**Owner:** Raven project  
**Created:** 2026-11-06  
**Canonical product roadmap:** `MILESTONES.md` (unchanged)  
**Related track:** `docs/roadmaps/UPSTREAM_REUSE_AND_ACCELERATION.md` (OpenCode/Kilo-focused; separate)

> This roadmap establishes a local, read-only reference library of open-source coding-agent CLIs and developer-agent projects. It does not authorize copying code into Raven, change Raven's M0–M8 milestones, or replace the focused OpenCode/Kilo acceleration roadmap.

## 1. Purpose

Build a curated local research library so Raven can learn from the strongest available open-source agent developer CLIs instead of independently rediscovering every design and implementation.

The library should help us:

- compare agent loops, tool systems, context construction, permissions, approvals, sessions, memory, MCP, skills, plugins, terminal UX and provider integrations;
- inspect real source code, tests, architecture and release history when a design question arises;
- identify reusable protocols, libraries and narrowly scoped implementations;
- decide whether Raven should integrate an external runtime, adapt a subsystem in Rust, implement the behavior independently, defer it, or reject it;
- accelerate development without weakening Raven's security, portability, local-first operation or maintainability.

The target is a **curated research library, not a collection of dependencies**.

## 2. Separation from the other Raven roadmaps

| Track | Responsibility | Boundary |
|---|---|---|
| Canonical Raven M0–M8 | Deliver Raven's own product | Remains the source of truth for product milestones |
| Upstream Reuse & Acceleration | Deep OpenCode/Kilo comparison and focused integration prototypes | Remains the dedicated OpenCode/Kilo workstream |
| OSS CLI Research & Local Source Library | Discover, clone, index and compare a wider set of open-source projects | Research-only until an explicit adoption decision |

Findings from this research track may recommend work for another track. They do not silently add scope to it. Any accepted implementation must be planned and reviewed in the canonical Raven roadmap or the focused acceleration track as appropriate.

## 3. Research scope and candidate catalogue

Start with projects that have inspectable source code and meaningful agent/developer workflow capabilities. The list below is a discovery shortlist, not a claim that every project is equally suitable, actively maintained, or approved for reuse.

### Tier 1 — Coding-agent CLIs and agent runtimes

| Project | Repository | Research focus |
|---|---|---|
| OpenCode | https://github.com/anomalyco/opencode | Agent runtime, providers, tools, sessions, TUI, MCP |
| Kilo Code | https://github.com/Kilo-Org/kilocode | OpenCode-derived runtime, CLI evolution, modes, extensions |
| Aider | https://github.com/Aider-AI/aider | Repository mapping, edit formats, workflows, model integration |
| Goose | https://github.com/block/goose | Extensible agent, MCP, recipes, local execution |
| Plandex | https://github.com/plandex-ai/plandex | Task planning, large changes, checkpoints and execution |
| Cline | https://github.com/cline/cline | Tool lifecycle, approvals, browser and editor-agent workflows |
| Roo Code | https://github.com/RooCodeInc/Roo-Code | Modes, orchestration, tools and task management |
| Continue | https://github.com/continuedev/continue | Context providers, model interfaces, indexing and extensions |
| Mentat | https://github.com/AbanteAI/mentat | Repository-aware coding assistance and change workflows |

### Tier 2 — Agent frameworks and developer automation

| Project | Repository | Research focus |
|---|---|---|
| OpenHands | https://github.com/All-Hands-AI/OpenHands | Agent execution architecture, runtime isolation, tools |
| SWE-agent | https://github.com/SWE-agent/SWE-agent | Issue-to-code workflow, repository interaction and evaluation |
| GPT-Engineer | https://github.com/AntonOsika/gpt-engineer | Specification-driven generation and project workflows |
| MetaGPT | https://github.com/FoundationAgents/MetaGPT | Multi-agent roles, task decomposition and coordination |
| AutoGen | https://github.com/microsoft/autogen | Agent communication and orchestration patterns |
| LangGraph | https://github.com/langchain-ai/langgraph | Stateful workflows, checkpointing and recovery |

### Tier 3 — Useful adjacent tools and components

Explore selectively where they solve a concrete Raven problem:

- terminal multiplexing and TUI libraries;
- command execution and process supervision;
- filesystem watchers and repository indexing;
- syntax-aware search and code parsing;
- MCP clients, servers and protocol SDKs;
- credential stores and secret-management libraries;
- sandboxing and OS isolation tools;
- structured logging, tracing and audit components;
- patch generation, diff review and undo systems.

Do not clone every adjacent dependency repository. Add a component only when it has a clear research question.

### Discovery policy

The catalogue is intentionally extensible. During R0, search for additional actively maintained open-source coding-agent CLIs and inspect their repository, license, language, runtime, platform support, activity and distinctive capabilities before adding them.

For each candidate, record why it belongs in the library. Avoid duplicates, abandoned projects without a specific historical research value, unrelated general-purpose frameworks and repositories with unclear source or licensing status.

## 4. Local directory layout

Keep the reference library **outside the Raven production repository**. Suggested Windows layout:

```text
C:\dev\
  raven-cli\
  raven-oss-lab\
    README.md
    catalogue.md
    inventory.json
    projects\
      opencode\
      kilocode\
      aider\
      goose\
      plandex\
      cline\
      roo-code\
      continue\
      mentat\
      openhands\
      swe-agent\
    comparisons\
    architecture-notes\
    extraction-candidates\
    prototypes\
    license-and-provenance\
```

Adjust the root path to the user's actual development directory. Do not put these repositories inside `raven-cli`, its Cargo workspace, or its release package.

### Clone and storage rules

- Use Git clones so commit history, tags and blame remain available.
- Start with shallow clones or partial/sparse checkouts when practical; deepen or expand only when the research question requires it.
- Record the exact upstream URL, branch, commit SHA, clone date and license evidence.
- Keep local reference clones read-only by convention; do not commit Raven changes into upstream working trees.
- Do not install every project's dependencies or build every project just because it was cloned.
- Do not copy `node_modules`, build outputs, caches, virtual environments, generated files or large model assets into the research library.
- Keep prototypes in the separate `prototypes/` directory or in isolated Raven branches.
- Never add the entire library as a Raven submodule, workspace member, release dependency or vendored source directory.

## 5. Provenance, license and security controls

A public repository is not automatically safe or reusable. Before any source is copied or adapted:

1. Verify the repository's current license and the license of the exact source file or package.
2. Inspect relevant dependency and asset licenses; a root license does not necessarily cover every bundled component.
3. Record upstream repository, immutable commit SHA, file paths, origin project and original copyright notices.
4. Identify local modifications and preserve required notices and attribution.
5. Check for security-sensitive behavior, telemetry, network calls, hosted services, credential handling and subprocess execution.
6. Record the Raven tests and security review required for the proposed adoption.

Do not assume all candidates use MIT or another permissive license. If licensing is absent, ambiguous, incompatible or not verified, mark the candidate **research-only / reuse blocked** until reviewed.

Never copy secrets, personal credentials, provider keys, tokens, private repository data or user configuration into the library or research notes. Do not run untrusted project scripts or install dependencies without inspecting what they execute.

## 6. Research workflow

### R0 — Local library bootstrap

**Goal:** create a manageable, reproducible source collection.

Tasks:

- create the local directory structure;
- clone the Tier 1 shortlist, subject to disk-space and license checks;
- record exact branch and commit identities;
- capture root license files and project metadata;
- produce a catalogue and inventory;
- identify overlap, especially Kilo's OpenCode-derived components;
- keep all source outside Raven's production tree.

**Exit criteria:**

- each cloned project has a recorded URL, branch, SHA, date, language/runtime and license status;
- failed or skipped clones are reported, not silently omitted;
- no source is imported into Raven;
- the local library can be updated or reproduced from its inventory.

### R1 — Comparative capability mapping

**Goal:** identify what is worth learning from each project.

Compare at least:

- agent loop and model/provider abstraction;
- tool schemas, dispatch and result handling;
- permission rules, approval UX and trust boundaries;
- command execution, process lifetime and sandboxing;
- context assembly, repository mapping and code search;
- sessions, persistence, compaction and recovery;
- MCP, skills, plugins and extension lifecycle;
- terminal interface and headless operation;
- cancellation, retries, error handling and observability;
- tests, packaging, installation and cross-platform support.

For every finding, include source paths, relevant tests, upstream SHA and a concise explanation. Distinguish observed implementation from inferred behavior.

**Exit criteria:**

- a capability comparison matrix exists;
- overlapping inherited implementations are not counted twice;
- high-value design patterns and candidate components are ranked;
- each candidate has a recommendation and evidence.

### R2 — Raven gap and reuse analysis

**Goal:** compare upstream capabilities with Raven's current implementation and roadmap.

For each candidate, record:

- Raven capability and current crate/module;
- upstream implementation and exact source location;
- functional gap and user value;
- language/runtime compatibility;
- dependency, binary-size and operational cost;
- Windows, Linux and macOS implications;
- security and policy implications;
- estimated adaptation effort;
- recommendation: integrate, adapt/port, independently implement, defer or reject.

**Exit criteria:** a prioritized backlog of evidence-based opportunities, with no automatic approval to import code.

### R3 — Isolated proof of concepts

**Goal:** test only the most promising approaches.

- choose a small number of high-value candidates;
- prototype behind an isolated interface or in a dedicated branch;
- compare implementation time against a Raven-native implementation;
- measure runtime overhead, memory, dependency footprint and portability;
- test cancellation, failure handling, permissions and auditability;
- document what the prototype does not secure or support.

No prototype may bypass Raven's policy, approval, redaction, workspace boundary or audit requirements. External runtimes remain disabled by default unless their actions can be mediated and their security boundary is explicitly understood.

### R4 — Selective adoption recommendations

**Goal:** submit focused proposals to the appropriate Raven roadmap.

Each proposal must include:

- the capability and user benefit;
- exact upstream source revision and paths;
- license and attribution plan;
- integration design and dependency impact;
- security review and tests;
- maintenance/upstream tracking strategy;
- explicit files and milestones affected.

Only separately accepted proposals may change Raven production code.

### R5 — Ongoing source maintenance

- periodically refresh the catalogue and upstream branch/commit references;
- record major architecture or license changes;
- retain old commit pins for reproducibility;
- re-run provenance checks before each extraction;
- prune research candidates only through an explicit decision, never by deleting user work.

## 7. Prioritization model

Score candidates from 1–5 for:

- relevance to Raven's product direction;
- amount of engineering time potentially saved;
- quality of implementation and tests;
- compatibility with Rust and Raven's architecture;
- security compatibility;
- portability;
- maintenance and dependency cost.

Prioritize high-value, low-coupling capabilities. A popular project or a large feature count is not sufficient reason to adopt it.

## 8. OpenCode execution instructions

Use OpenCode as the local research orchestrator after the user has cloned this roadmap branch and created the local library directory. OpenCode should inspect, catalogue and compare sources; it must not import code into Raven or alter upstream clones.

### Prompt A — Bootstrap and inventory

```text
You are working on the Raven OSS CLI Research & Local Source Library track.

Read docs/roadmaps/OSS_CLI_SOURCE_LIBRARY_ROADMAP.md in the Raven repository first. The local research library must be outside the Raven repository, for example C:\dev\raven-oss-lab. Ask me for the actual path if it is not present.

Task:
1. Inspect available disk space and existing folders before cloning.
2. Create the directory layout specified by the roadmap.
3. Clone the Tier 1 repositories listed in the roadmap using shallow clones where practical. Do not clone duplicates or overwrite existing folders. If a repository already exists, inspect its remote and working-tree state first.
4. Do not install dependencies, execute project scripts, build projects, or run downloaded binaries.
5. Record each repository URL, default/reference branch, checked-out commit SHA, clone date, primary language/runtime, root license and any obvious subproject license files.
6. Identify forks and inherited code overlap, especially Kilo and OpenCode.
7. Create catalogue.md and inventory.json in the research library.
8. Report skipped, failed, ambiguous-license and low-disk-space cases explicitly.

Safety:
- Research only. Do not modify Raven source, tests, manifests, lockfiles, or roadmap documents.
- Do not copy any upstream code into Raven.
- Do not expose or collect credentials.
- Do not delete, reset, clean or overwrite existing work.
- Do not claim a clone or license check succeeded unless verified.
Return exact repository paths, branches, SHAs, license findings, disk usage and remaining work.
```

### Prompt B — Architecture comparison

```text
Continue the Raven OSS CLI Research & Local Source Library roadmap.

Read the roadmap and the local catalogue first. Select the most relevant coding-agent projects and inspect their source at the exact pinned commits.

Compare their architecture and implementation for:
- agent loop and provider adapters;
- tool contracts and execution lifecycle;
- permissions, approvals and trust;
- command execution and sandboxing;
- context management and repository indexing;
- sessions, persistence and recovery;
- MCP, skills and plugins;
- terminal UX and headless operation;
- testing, packaging and cross-platform support.

For each finding:
1. Give the exact repository, commit SHA, file paths and relevant tests.
2. Explain what the code demonstrably does; separate inference from verified behavior.
3. Compare it with Raven's existing crates and M0-M8 milestones.
4. Identify OpenCode/Kilo inherited overlap to avoid duplicate counting.
5. Estimate native Rust adaptation versus optional runtime integration.
6. Assess license, security, portability, dependency and maintenance costs.
7. Recommend integrate, adapt, reimplement, defer or reject.

Produce comparison reports under the local research library's comparisons directory. Do not change Raven production code or import upstream files. Do not run untrusted scripts or install dependencies. Finish with a ranked shortlist of the five highest-value opportunities and the evidence for each.
```

### Prompt C — One-component extraction feasibility study

```text
Choose one approved candidate from the research comparison and perform a read-only extraction feasibility study.

Do not copy or modify code yet.

Trace the upstream entry points, interfaces, dependencies, tests, license notices and security assumptions. Map the behavior to Raven's current Rust crates. Identify whether the best path is a protocol boundary, optional external runtime, narrow Rust port or independent implementation.

Provide:
- exact upstream provenance;
- a proposed Raven interface and data flow;
- files/crates likely to be affected;
- security and cross-platform risks;
- required tests and acceptance criteria;
- estimated complexity and maintenance burden;
- a go/no-go recommendation.

Do not edit Raven files, create commits, open PRs or change upstream repositories.
```

## 9. Governance and non-goals

This track does not authorize:

- wholesale copying or merging of OpenCode, Kilo or other repositories;
- importing upstream code without file-level license and provenance review;
- weakening Raven's fail-closed security model for feature parity;
- adding mandatory Node/Bun/Python runtimes to Raven without a product decision;
- executing downloaded code merely to inspect it;
- changing canonical milestones, source code, tests or release claims;
- creating PRs, merging branches or publishing a release.

The user may direct OpenCode to execute the research prompts locally. The resulting reports should be reviewed before any implementation work is approved.

## 10. Definition of success

The research library is successful when Raven maintainers can answer, with source-level evidence:

- Which existing project already solves this problem well?
- Is it actually reusable, and under what license?
- What does it cost to integrate and maintain?
- Does it preserve Raven's security and portability requirements?
- Is integration faster and safer than a focused Rust implementation?
- Where should the capability live in Raven's architecture and roadmap?

**Guiding principle:** study broadly, compare rigorously, prototype narrowly, adopt selectively. Raven remains its own product and security authority.
