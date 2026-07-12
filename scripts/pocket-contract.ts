// Shared manifest/plan boundary for OpenStrike's custom native hosts.
//
// PocketJS owns validation, target capability resolution, target-specific
// TypeScript checks, and JS/pak compilation. OpenStrike consumes the resolved
// plan only to bind that exact bundle contract into its custom 3D host.

import { $ } from "bun";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import {
  verifyBuildPlanHash,
  type ResolvedBuildPlan,
} from "@pocketjs/framework/manifest";

const repo = resolve(new URL("..", import.meta.url).pathname);
export type OpenStrikeBuildPlan = ResolvedBuildPlan;

export function pocketOutputDirectory(target: string): string {
  return resolve(repo, "dist", "pocket", target);
}

function frameworkRoot(): string {
  const root = resolve(`${repo}/vendor/pocketjs`);
  if (!existsSync(`${root}/scripts/pocket.ts`)) {
    throw new Error(
      `PocketJS platform-contract toolchain not found at ${root}; initialize or update vendor/pocketjs`,
    );
  }
  return root;
}

/** Resolve/check a target, compile from its immutable plan, then return it. */
export async function compilePocketTarget(target: string): Promise<OpenStrikeBuildPlan> {
  const framework = frameworkRoot();
  const manifest = `${repo}/pocket.json`;
  const planPath = `${repo}/.pocket/${target}/plan.json`;
  const outdir = pocketOutputDirectory(target);

  await $`bun ${framework}/scripts/pocket.ts compile --target ${target} --manifest ${manifest} --project-root ${repo} --outdir ${outdir}`.cwd(
    repo,
  );

  const plan = await Bun.file(planPath).json() as ResolvedBuildPlan;
  if (!verifyBuildPlanHash(plan)) {
    throw new Error(`PocketJS produced an invalid or stale plan at ${planPath}`);
  }
  if (plan.target.id !== target) {
    throw new Error(`PocketJS plan target mismatch: expected ${target}, got ${plan.target.id}`);
  }
  return plan;
}

/** Environment shared by the framework host dependency and custom primary crate. */
export function nativePocketContract(plan: OpenStrikeBuildPlan): Record<string, string> {
  return {
    POCKETJS_APP_OUTPUT: plan.app.output,
    POCKETJS_EMBED_APP: "0",
    POCKETJS_TARGET: plan.target.id,
    POCKETJS_HOST_ABI: String(plan.target.hostAbi),
    POCKETJS_CONTRACT_HASH: plan.contractHash,
    POCKETJS_LOGICAL_WIDTH: String(plan.viewport.logical[0]),
    POCKETJS_LOGICAL_HEIGHT: String(plan.viewport.logical[1]),
    POCKETJS_PHYSICAL_WIDTH: String(plan.viewport.physical[0]),
    POCKETJS_PHYSICAL_HEIGHT: String(plan.viewport.physical[1]),
    POCKETJS_OUTPUT_DIR: pocketOutputDirectory(plan.target.id),
  };
}
