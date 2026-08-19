# sidecar-dist

占位目录：`scripts/assemble-sidecar.mjs`（阶段 2）会在此产出随包分发的 sidecar：

- `node/` — Node 运行时（钉版，仅随包分发）
- `pnpm/` — pnpm standalone（`pnpm.cjs` + `pnpm.cmd` shim）
- `node_modules/` — 钉死版本的 `@deepseek-ai/dsh` 依赖树

本 README 之外的产物一律不入库（见根 `.gitignore`）。
