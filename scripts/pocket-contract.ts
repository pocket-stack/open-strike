// Shared manifest/plan boundary for OpenStrike's custom native hosts.
//
// PocketJS owns validation, target capability resolution, ordinary reachable
// TypeScript checks, and JS/pak compilation. OpenStrike consumes only the
// small stable host projection of the internal resolved plan.

import { $ } from "bun";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import {
  extractHostBuildInputs,
  hostBuildEnvironment,
  type HostBuildInputs,
} from "@pocketjs/framework/manifest";

const repo = resolve(new URL("..", import.meta.url).pathname);

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

/** Resolve/check a target, compile its app, then return stable host inputs. */
export async function compilePocketTarget(target: string): Promise<HostBuildInputs> {
  const framework = frameworkRoot();
  const manifest = `${repo}/pocket.json`;
  const planPath = `${repo}/.pocket/${target}/plan.json`;
  const outdir = pocketOutputDirectory(target);

  await $`bun ${framework}/scripts/pocket.ts compile --target ${target} --manifest ${manifest} --project-root ${repo} --outdir ${outdir}`.cwd(
    repo,
  );

  const plan: unknown = await Bun.file(planPath).json();
  return extractHostBuildInputs(plan, { expectedTarget: target });
}

/** Environment shared by the framework host dependency and custom primary crate. */
export function nativePocketContract(inputs: HostBuildInputs): Readonly<Record<string, string>> {
  return hostBuildEnvironment(inputs, {
    outputDirectory: pocketOutputDirectory(inputs.target),
    embedApp: false,
  });
}
