# Codebase Navigation (Qwen-Local)

## When to use this skill
Triggered when working on `navigator-agent`, `codebase-indexer`, or any code that exposes tools to the navigator LLM. Use when the agent needs to explore a repository it has never seen before.

## Core concepts
1. **Budget is Sacred (Qwen Edition)**: Qwen3.5 4B has a 32k context window. Use it wisely. Prefer reading small chunks (max 200 lines) over whole files. 
2. **Evidence Before Claims**: Never propose a claim without a `SourceRef`. A claim without evidence is a hallucination.
3. **Follow the Data**: Use the call graph and import graph provided by `codebase-indexer`. Don't rely on directory structure alone.
4. **Framework Priors**: React/Flutter/SwiftUI have standard patterns. Use the framework-specific guides to find entrypoints (routes, state stores).
5. **Calibrated Uncertainty**: If Qwen isn't sure, it should say so. Low-confidence claims are better than confident lies.

## Conventions
- Tool results must be minimal. `read_file` requires a `line_range`.
- Tool names: `list_files`, `read_file`, `grep`, `find_symbol`, `trace_imports`, `propose_claim`.
- Max lines per `read_file`: 200.
- Max matches per `grep`: 50.

## Common pitfalls
- **Reading whole files**: Qwen will try to consume the whole file. Stop it by requiring ranges.
- **Circular Imports**: `trace_imports` must handle cycles.
- **Generated Noise**: Always ignore `node_modules`, `build`, `.dart_tool`, etc.
- **Test files**: Distinguish between `src` and `test`. Tests provide context but shouldn't be the primary source for "Feature" claims unless verified.

## Examples
**Bad (No Evidence)**:
```json
{ "tool": "propose_claim", "args": { "statement": "User can log in", "confidence": 0.9 }}
```
**Good (With Evidence)**:
```json
{ "tool": "propose_claim", "args": {
  "statement": "App supports GitHub OAuth login",
  "confidence": 0.85,
  "evidence": [{
    "file": "src/auth/github.rs",
    "line_start": 10,
    "line_end": 45,
    "reason": "Found GitHubProvider configuration and callback handler."
  }]
}}
```
