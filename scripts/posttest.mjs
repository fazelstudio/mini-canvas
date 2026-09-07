#!/usr/bin/env bun
// posttest hook — auto-updates benchmarks/RESULTS.md when BENCH_UPDATE=1
// Usage: BENCH_UPDATE=1 bun run test   → test + bench update in one go
//        bun run test:update            → same, explicit
// No-op otherwise (zero cost).

import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

if (process.env.BENCH_UPDATE === "1" || process.env.UPDATE_BENCH === "1") {
  console.log("\n[posttest] BENCH_UPDATE=1 → updating benchmarks/RESULTS.md ...");
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const bench = resolve(root, "benchmarks/bench.mjs");
  const result = spawnSync("bun", [bench, "--update"], { stdio: "inherit", cwd: root });
  if (result.status !== 0 && result.status !== null) process.exit(result.status);
}
