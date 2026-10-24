// Linux counterpart of prepare-sidecar.mjs: verify the staged Linux
// sidecars against the build-time pins before Tauri bundles anything.
// The pins (src-tauri/linux-binary-pins.json) are produced by
// scripts/fetch-core-linux.sh from digest-verified upstream archives;
// this step re-hashes what is on disk so a swapped or stale binary
// fails the build here, exactly like the Windows packaging check.
import { createHash } from "node:crypto";
import { createReadStream, existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";

const target = "x86_64-unknown-linux-gnu";
const pinsPath = resolve("src-tauri/linux-binary-pins.json");

if (!existsSync(pinsPath) && (process.env.AETHER_LINUX_SHA256 || process.env.XRAY_LINUX_SHA256)) {
  // Package builds (PKGBUILD) export pins via the environment after their
  // own verified fetch; the runtime hash check still applies.
  console.log("Using environment-provided Linux binary pins (package build)");
  process.exit(0);
}
if (!existsSync(pinsPath)) {
  throw new Error("Linux sidecars are not staged. Run `npm run fetch:linux` first.");
}

const pins = JSON.parse(readFileSync(pinsPath, "utf8"));
const expected = {
  aether: process.env.AETHER_LINUX_SHA256 || pins.aether,
  xray: process.env.XRAY_LINUX_SHA256 || pins.xray,
};

async function sha256(path) {
  const hash = createHash("sha256");
  await new Promise((done, fail) => {
    createReadStream(path)
      .on("data", (chunk) => hash.update(chunk))
      .on("error", fail)
      .on("end", done);
  });
  return hash.digest("hex");
}

for (const name of ["aether", "xray"]) {
  const path = resolve("src-tauri/binaries", `${name}-${target}`);
  if (!existsSync(path)) {
    throw new Error(`Bundled ${name} is missing at ${path}. Run \`npm run fetch:linux\`.`);
  }
  const actual = await sha256(path);
  if (actual !== expected[name]) {
    throw new Error(
      `Bundled ${name} digest mismatch: expected ${expected[name]}, got ${actual}. Re-run \`npm run fetch:linux\`.`,
    );
  }
  console.log(`Using bundled ${name} at ${path} (SHA-256 verified)`);
}

for (const helper of ["psiphon-tunnel-core", "lyrebird"]) {
  const path = resolve("src-tauri/binaries/pt", helper);
  if (!existsSync(path)) {
    throw new Error(`Privacy helper pt/${helper} is missing. Run \`npm run fetch:linux\`.`);
  }
  const wanted = pins.pt?.[helper];
  if (wanted) {
    const actual = await sha256(path);
    if (actual !== wanted) {
      throw new Error(`pt/${helper} digest mismatch: expected ${wanted}, got ${actual}.`);
    }
    console.log(`Using staged pt/${helper} (SHA-256 verified)`);
  }
}
