// Derive the explicitly reduced E7 package from the canonical Pocket
// contract. Keeping this mechanical avoids a second hand-maintained manifest
// drifting from OpenStrike's version, capabilities, or viewport policy.

interface PocketLikeManifest {
  $schema?: unknown;
  pocket?: unknown;
  id?: unknown;
  name?: unknown;
  title?: unknown;
  version?: unknown;
  engine?: {
    capabilities?: {
      requires?: unknown;
      enhances?: unknown;
      [key: string]: unknown;
    };
    [key: string]: unknown;
  };
  app?: {
    entry?: unknown;
    output?: unknown;
    framework?: unknown;
    viewport?: unknown;
    [key: string]: unknown;
  };
  [key: string]: unknown;
}

export const SYMBIAN_COMPAT_ENTRY = "game/openstrike-symbian.tsx";
export const SYMBIAN_COMPAT_OUTPUT = "openstrike-e7-compat";
export const SYMBIAN_COMPAT_LIVE_VIEWPORT = "display.viewport.live";
export const SYMBIAN_COMPAT_DYNAMIC_VIEWPORT = {
  default: [640, 360],
  min: [360, 360],
  max: [640, 640],
} as const;

export function deriveSymbianCompatManifest(input: unknown): PocketLikeManifest {
  if (
    input === null ||
    typeof input !== "object" ||
    Array.isArray(input)
  ) {
    throw new Error("canonical pocket.json must be an object");
  }
  const canonical = input as PocketLikeManifest;
  const capabilities = canonical.engine?.capabilities;
  const enhances = capabilities?.enhances;
  const viewport = canonical.app?.viewport;
  if (
    canonical.pocket !== 2 ||
    typeof canonical.id !== "string" ||
    typeof canonical.name !== "string" ||
    typeof canonical.title !== "string" ||
    typeof canonical.version !== "string" ||
    canonical.app === undefined ||
    canonical.app.entry !== "game/openstrike.tsx" ||
    capabilities === undefined ||
    !Array.isArray(enhances) ||
    !enhances.every((capability) => typeof capability === "string") ||
    viewport === null ||
    typeof viewport !== "object" ||
    Array.isArray(viewport) ||
    !("fixed" in viewport) ||
    "dynamic" in viewport
  ) {
    throw new Error(
      "canonical pocket.json identity/version/capabilities/fixed viewport is incomplete or unexpected",
    );
  }

  const derived = structuredClone(canonical);
  derived.id = `${canonical.id}.e7-compat`;
  derived.name = "open-strike-e7-compat";
  derived.title = "OpenStrike E7 2D Compat";
  derived.engine = {
    ...derived.engine!,
    capabilities: {
      ...derived.engine!.capabilities!,
      enhances: [
        ...enhances,
        ...(enhances.includes(SYMBIAN_COMPAT_LIVE_VIEWPORT)
          ? []
          : [SYMBIAN_COMPAT_LIVE_VIEWPORT]),
      ],
    },
  };
  derived.app = {
    ...derived.app!,
    entry: SYMBIAN_COMPAT_ENTRY,
    output: SYMBIAN_COMPAT_OUTPUT,
    viewport: {
      ...(structuredClone(viewport) as Record<string, unknown>),
      dynamic: structuredClone(SYMBIAN_COMPAT_DYNAMIC_VIEWPORT),
    },
  };
  return derived;
}
