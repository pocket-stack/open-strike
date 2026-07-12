import { compilePocketTarget, pocketOutputDirectory } from "./pocket-contract.ts";

const argv = Bun.argv.slice(2);
const targetIndex = argv.indexOf("--target");
const target = targetIndex >= 0 ? argv[targetIndex + 1] : undefined;
if (!target || argv.length !== 2 || targetIndex !== 0) {
  console.error("usage: bun scripts/build-ui.ts --target <target>");
  process.exit(1);
}

const plan = await compilePocketTarget(target);
console.log(
  `output: ${pocketOutputDirectory(target)}/${plan.app.output}.{js,pak} (${plan.contractHash})`,
);
