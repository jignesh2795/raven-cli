# Raven Upstream Reuse & Acceleration Roadmap

**Status:** Proposed research and engineering track  
**Owner:** Raven project  
**Last reviewed:** 2026-11-06

> This document is a separate supporting roadmap. It does not replace, renumber,
> or silently modify the canonical M0–M8 plan in `MILESTONES.md`. Its purpose is
> to help Raven deliver capabilities faster by studying and selectively reusing
> proven work from OpenCode and Kilo Code.

## 1. Purpose

Reduce Raven implementation time without sacrificing Raven's Rust architecture,
local-first direction, security model, auditability, or platform portability.

The project will examine two upstream codebases:

- OpenCode: https://github.com/anomalyco/opencode
- Kilo Code: https://github.com/Kilo-Org/kilocode

The intended outcome is not a wholesale replacement of Raven. It is a
component-by-component decision about what to:

1. integrate as an optional external runtime;
2. adapt or port into Raven's Rust crates;
3. reuse through a stable protocol or library boundary; or
4. learn from and implement independently.

Every adoption must have a clear capability benefit, a maintenance plan, an
origin record, license review, and Raven-specific security tests.

## 2. Key research findings and repository baselines

The initial source review on 2026-11-06 found:

| Project | Reference branch | Inspected commit | Language / toolchain | License |
| --- | --- | --- | --- | --- |
| OpenCode | `dev` | `a7515ce7e6cb67043e335b80f140c3bb241439dc` | TypeScript / Bun | MIT |
| Kilo Code released line | `main` | `b8fb3b844a519c3f68ada86d9371b90aded9f3c5` | TypeScript / Bun | MIT |
| Kilo Code development line | `v2` | `72eff8a14f0bab9ebd46f535797e84abc92194dc` | TypeScript / Bun | MIT |

These are research baselines, not permanent pins. Re-check upstream branches
and licenses before every extraction or integration decision.

### Important relationship: Kilo is derived from OpenCode

Kilo's own README identifies Kilo CLI as a fork of OpenCode. The Kilo
repository also contains an OpenCode-derived runtime under
`packages/opencode`, alongside Kilo packages and platform-specific product
surfaces.

Therefore:

- OpenCode is the primary upstream reference for the shared coding-agent
  foundation.
- Kilo is the downstream comparison for its deltas, enhancements, packaging,
  CLI evolution, and additional subsystems.
- Do not count the same inherited OpenCode capability twice.
- Compare Kilo's released `main` and active `v2` development line where their
  behavior differs. The Kilo README currently directs new feature work to
  `v2`, not `main`.

### Compatibility constraint

Raven is implemented in Rust. OpenCode and Kilo are TypeScript/Bun monorepos.
Their source cannot generally be copied into Raven's Rust crates and expected
to compile. Reuse therefore means one of three deliberate approaches:

- **Runtime integration:** invoke a separately installed OpenCode/Kilo runtime
  through a documented process or service boundary.
- **Rust adaptation:** translate a selected subsystem's behavior and contracts
  into Raven-native Rust, preserving upstream attribution when code is derived.
- **Protocol/library integration:** use a stable, reviewed protocol or
  independently packageable component where that is more maintainable than a
  port.

A language rewrite is not automatically faster than a focused native
implementation. Compare the approaches with a small proof of concept before
committing to a large extraction.

## 3. Repository and source-management policy

Keep upstream reference source separate from Raven's production source tree.

Recommended working layout (outside the Raven repository):

```text
raven-workspace/
  raven-cli/
  upstream-research/
    opencode/
    kilocode/
    source-inventory.md
    comparison-notes/
    prototypes/
```

GitHub forks under the Raven maintainer's account may be used to preserve
long-lived experiments and upstream tracking. Local reference clones should be
pinned to reviewed commits and use shallow or sparse checkout initially to
avoid downloading or building entire monorepos unnecessarily.

Do not add the complete OpenCode or Kilo repositories as Raven git submodules,
nested repositories, or mandatory runtime dependencies. Do not vendor entire
upstream monorepos into the Raven production tree.

For each adopted component, record:

- upstream repository, branch, commit SHA, and source file paths;
- whether it originated in OpenCode, Kilo, or a third-party dependency;
- the original license and required notices;
- local changes and the Raven commit that imported or adapted it;
- dependencies and their licenses;
- security review, tests, and upstream update strategy.

## 4. Candidate capability areas

These are research candidates, not approval to import every feature.

| Capability | OpenCode research focus | Kilo research focus | Raven destination / decision |
| --- | --- | --- | --- |
| Model and provider ecosystem | Provider adapters, model catalog, auth flows, streaming behavior | Kilo provider and gateway deltas | `raven-provider`; compare native Rust adapters with an optional external provider bridge |
| Agent loop and tool execution | Tool contracts, tool-call lifecycle, cancellation, context handling | CLI runtime changes and tool-schema handling | `raven-core`, `raven-tools`; Raven's policy and approval layer remains authoritative |
| Permissions and rules | Permission requests, grants, denials, rule matching | Modes and permission workflow deltas | `raven-core`; compare with Raven's fail-closed ruleset before adapting |
| Sessions and context | Session history, compaction, recovery, conversation persistence | Session UX and additional memory features | `raven-store`, `raven-core`; preserve Raven's event log as the audit source of truth |
| MCP | Client lifecycle, local/remote servers, tool registration | Kilo MCP integration and marketplace changes | M5 / future MCP crate; no project MCP server is trusted automatically |
| Skills and plugins | Plugin hooks, extension lifecycle, tool registration | Skills, agents, marketplace and plugin deltas | M7/M8; preserve explicit trust, permissions, provenance and isolation |
| Code understanding | Search, file watching, syntax and context construction | Indexing and memory packages | `raven-tools` or a later indexing crate; measure value before adding a database/index |
| Terminal UX | TUI architecture, rendering, input and session controls | CLI/TUI and editor-client experience | Later Raven CLI/TUI work; port interaction contracts, not UI wholesale |
| Runtime interoperability | Server, protocol and SDK boundaries | CLI/server and SDK changes | New optional runtime-adapter boundary after process execution is secure |

Prioritize capabilities that remove substantial engineering work and fit
Raven's architecture. Avoid importing broad features whose dependencies,
operational assumptions, hosted services, or security model would cost more
than implementing the narrow Raven requirement.

## 5. Roadmap phases

### R0 — Source acquisition and provenance inventory

**Goal:** establish reproducible, low-cost access to the upstream references.

Tasks:

- Keep a reference clone or maintainer fork of OpenCode.
- Keep a reference clone or maintainer fork of Kilo Code.
- Pin the reviewed OpenCode `dev`, Kilo `main`, and Kilo `v2` baselines.
- Record repository size, runtime requirements, build requirements, package
  boundaries and license files.
- Create a source inventory identifying the Kilo/OpenCode overlap.
- Use shallow/sparse checkout before full dependency installation.

**Exit criteria:**

- Source baselines are reproducible by commit SHA.
- Overlapping and Kilo-specific subsystems are identified.
- Root and relevant package licenses are captured.
- No upstream source has been added to Raven's production crates.

### R1 — Comparative architecture and feature extraction study

**Goal:** discover the highest-value reusable capabilities and the best
integration method for each.

For every candidate subsystem:

1. Trace its entry point, contracts, data flow, dependencies and tests.
2. Compare OpenCode with the corresponding Kilo implementation.
3. Map the capability to an existing Raven crate or identify a justified new
   adapter boundary.
4. Identify policy, trust, secrets, audit, platform and offline implications.
5. Estimate native Rust adaptation versus external runtime integration.
6. Record a recommendation: integrate, port, reimplement, defer, or reject.

**Exit criteria:**

- A prioritized feature comparison matrix exists.
- Each candidate has a reuse decision and rationale.
- No component is selected solely because it exists upstream.

### R2 — Small integration proofs of concept

**Goal:** validate the fastest sustainable reuse path before large changes.

Build isolated prototypes for:

- one optional external coding-agent runtime adapter;
- one selected Rust-native subsystem adaptation;
- one protocol or SDK boundary where a language-neutral integration is viable.

Compare:

- implementation effort;
- runtime and memory overhead;
- Windows compatibility;
- portability to Linux and macOS;
- error handling and cancellation;
- audit and policy enforcement;
- upstream version coupling and update cost.

**Hard security gate:** an external runtime may execute tools or commands outside
Raven's current controls. Do not enable delegation until Raven's command runner,
process lifecycle, workspace restrictions, permission mediation, and audit
integration can prevent that runtime from bypassing Raven policy. Any external
runtime remains disabled by default until explicitly configured and validated.

**Exit criteria:**

- Prototype results are documented with exact source revisions.
- A chosen integration pattern is supported by evidence.
- No prototype weakens Raven's existing fail-closed guarantees.

### R3 — Selective extraction and Raven-native adaptation

**Goal:** implement only the selected, high-value capabilities.

Rules:

- Keep `raven-core` platform-neutral and provider-neutral.
- Put OS-specific behavior behind platform adapters.
- Keep Raven's own policy, approvals, redaction, secret handling and audit
  semantics authoritative.
- Prefer narrow, independently testable modules over copying entire packages.
- Preserve upstream copyright and MIT license notices for copied or adapted
  code.
- Maintain a third-party notices and dependency-license inventory.
- Do not import cloud control planes, telemetry, billing or hosted gateway
  assumptions unless a separately approved product decision requires them.

**Exit criteria:**

- Every imported/adapted component has provenance and license records.
- Functional parity tests cover the intended behavior.
- Security tests prove that the integration does not bypass Raven controls.
- Dependency and binary-size impact is measured.
- Existing Raven tests and platform behavior remain green.

### R4 — Optional runtime interoperability

**Goal:** allow Raven to delegate work to compatible external coding-agent
runtimes when doing so adds value.

Potential design:

```text
Raven CLI
  |
Runtime Adapter Interface
  |-----------------------------|
  |                             |
Raven Native Agent       External Runtime Adapter
  |                             |
Raven Core/Policy        OpenCode or Kilo Process/Service
  |                             |
Raven Tools/Store        Explicitly mediated capabilities
```

This is an optional capability, not a replacement for Raven's native runtime.
It must have:

- explicit user configuration and consent;
- a documented capability and permission mapping;
- constrained workspace and environment;
- bounded process lifetime, output and cancellation;
- session correlation and auditable delegation records;
- clear disclosure of which security controls are enforced by Raven and which
  remain the responsibility of the external runtime.

**Exit criteria:**

- A delegated task cannot silently widen Raven's permissions.
- The user can identify which runtime performed each action.
- Cancellation and failure recovery are tested.
- External runtime integration is optional and does not make Bun/Node a
  mandatory dependency of the native Raven binary.

### R5 — Upstream tracking and maintenance

**Goal:** prevent extracted components from becoming unmaintainable forks.

Tasks:

- Periodically compare upstream changes against the pinned source baselines.
- Track security fixes and breaking API/schema changes.
- Maintain a small, explicit Raven delta for each adopted component.
- Re-run license, dependency, compatibility and security reviews before updates.
- Remove integrations whose maintenance cost exceeds their value.

**Exit criteria:**

- Every adopted component has a named upstream and update procedure.
- Upstream changes are not merged blindly.
- Raven can continue building if an optional upstream runtime is unavailable.

## 6. License and supply-chain requirements

The initial repository metadata and root license files identify OpenCode and
Kilo Code as MIT-licensed. MIT permits reuse subject to retaining the required
copyright and permission notices. Kilo's license file includes both Kilo Code
and OpenCode copyright notices.

This is not a blanket license conclusion for every file or dependency in
either monorepo. Before extraction:

- inspect the applicable license and notices for the exact source;
- inventory bundled assets, generated files and third-party code;
- review dependency licenses and native binaries separately;
- preserve required attributions in Raven;
- record source commit and file provenance;
- produce or update a software bill of materials when distributable artifacts
  include the adopted components.

No proprietary hosted service, paid gateway, API account or cloud dependency is
assumed to be reusable merely because its client code appears in an MIT
repository.

## 7. Non-goals

This acceleration track does not authorize:

- replacing Raven's Rust core with a wholesale TypeScript/Bun fork;
- copying both monorepos into Raven and maintaining duplicate source trees;
- bypassing Raven's policy, approval, redaction or event-store controls;
- enabling arbitrary third-party runtimes before process isolation is ready;
- importing hosted Kilo/OpenCode cloud infrastructure, billing or telemetry by
  default;
- claiming cross-platform support without native build and runtime validation;
- changing the canonical M0–M8 roadmap without an explicit roadmap decision.

## 8. Relationship to the canonical Raven roadmap

This document is a parallel research and acceleration plan:

- `MILESTONES.md` remains the canonical delivery roadmap.
- R0–R5 track upstream research, prototypes, extraction and maintenance.
- Accepted work from this track is assigned to the appropriate existing Raven
  milestone or to a separately approved future milestone.
- The acceleration track must not become a second competing implementation
  backlog.
- The current security-validation and remediation priorities remain in force.

## 9. Immediate next actions

1. Complete the current Raven Batch 2 remediation validation. Do not let upstream
   experimentation distract from the open security findings.
2. Create the two upstream reference clones/forks outside the Raven production
   checkout, pinned to the baselines in Section 2.
3. Produce the OpenCode-versus-Kilo-versus-Raven subsystem comparison.
4. Select no more than three high-value proofs of concept.
5. Make an evidence-based decision about native porting versus optional runtime
   interoperability before any source extraction.

**Current status:** roadmap only. No OpenCode or Kilo source has been imported
into Raven by this document.
