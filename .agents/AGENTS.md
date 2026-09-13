# AGENTS.md — Repository-Local Agent Guidance

1. This file is repository-local guidance for agents working in this workspace.

2. Start with `docs/README.md` and descend through its linked documents in order.
   Treat `docs/archive/` as historical only; do not rely on it for current behavior.

3. Respect the docs boundary: `docs/` contains durable
   architecture/contracts/decisions; detailed module contracts are in
   `src/**/README.md` and per-binary READMEs; operational verification is in
   `docs/engineering/quality-gates.md`. Read detail at its source.

4. Keep generated Rust outputs out of the repo root: use Cargo-managed builds so
   outputs stay under `target/`; if direct `rustc` is unavoidable, run it outside
   the repo or set an explicit temporary `--out-dir`/`-o`; never leave or commit
   root-level `.rlib`, `.rmeta`, or compiler-probe artifacts; remove accidental
   generated root artifacts immediately.

5. Stay concise and link rather than duplicating volatile details: keep this
   guidance short and link to `docs/` instead of copying commands, paths, or
   versioned behavior into agent files.
