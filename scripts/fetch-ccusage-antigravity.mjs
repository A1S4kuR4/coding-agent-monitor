// Build and stage the temporary Antigravity compatibility sidecar.
//
// Upstream ccusage 20.0.20 does not yet include Antigravity. We therefore pin
// PR #1487 at an exact commit and invoke this binary only for the focused
// `antigravity daily` report; the released 20.0.20 binary remains authoritative
// for the unified snapshot and every other agent.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { gunzipSync } from "node:zlib";

const TARGET_TRIPLE = "x86_64-pc-windows-msvc";
const PR_NUMBER = 1487;
const COMMIT = "c58c1b3aab2eacc82add250c8229bb6192e4489b";
const VERSION = "20.0.18-antigravity-c58c1b3";
const ARCHIVE_URL =
  `https://github.com/sambitcreate/ccusage/archive/${COMMIT}.tar.gz`;
const ARCHIVE_SHA512 =
  "f42cdf6ac8e9f375aa0cccfd97e0019166d4b331f71b8f02bb206bd4f038b7dda43b593c81488a5d0f2ae8677a32092c276f31f3308c72c87374a948648ec57e";
const ARCHIVE_PREFIX = `ccusage-${COMMIT}/`;
// The PR's build embeds this exact LiteLLM revision from its flake.lock.
const PRICING_URL =
  "https://raw.githubusercontent.com/BerriAI/litellm/34561482ed092d78c296cab7999486022af5a938/model_prices_and_context_window.json";
const PRICING_SHA512 =
  "0539458a2b33b3cd2eec9dc239585b98638089981fee4be8fc131c209f60b8d83a1d7cec7a7165c62b25fad220712731a68c265700cd98a7d5402077725c85a5";

const here = dirname(fileURLToPath(import.meta.url));
const destination = join(
  here,
  "..",
  "src-tauri",
  "binaries",
  `ccusage-antigravity-${TARGET_TRIPLE}.exe`,
);

function fail(message) {
  throw new Error(`[fetch-ccusage-antigravity] ${message}`);
}

function digest(buffer, algorithm) {
  return createHash(algorithm).update(buffer).digest("hex");
}

function isExpectedSidecar(path) {
  if (!existsSync(path)) return false;
  const version = spawnSync(path, ["--version"], { encoding: "utf8" });
  if (
    version.status !== 0 ||
    version.stdout.trim() !== `ccusage ${VERSION}`
  ) {
    return false;
  }
  const help = spawnSync(path, ["antigravity", "--help"], {
    encoding: "utf8",
  });
  return help.status === 0 && help.stdout.includes("Usage reports for antigravity");
}

async function downloadVerified(url, expectedSha512, label) {
  const response = await fetch(url);
  if (!response.ok) fail(`failed to download ${label}: HTTP ${response.status}`);
  const payload = Buffer.from(await response.arrayBuffer());
  const actual = digest(payload, "sha512");
  if (actual !== expectedSha512) {
    fail(`${label} SHA-512 mismatch\n  expected: ${expectedSha512}\n  actual:   ${actual}`);
  }
  return payload;
}

// Extract regular USTAR files. Symlinks in the GitHub archive are not required
// to compile the Rust workspace and are deliberately ignored.
function extractTar(tarBuffer) {
  const files = new Map();
  let offset = 0;
  let pendingName = null;
  const readField = (block, start, length) => {
    const nul = block.indexOf(0, start);
    const end = nul === -1 || nul > start + length ? start + length : nul;
    return block.subarray(start, end).toString("utf8");
  };

  while (offset + 512 <= tarBuffer.length) {
    const block = tarBuffer.subarray(offset, offset + 512);
    offset += 512;
    if (block.every((byte) => byte === 0)) break;
    const name = readField(block, 0, 100);
    const size = Number.parseInt(readField(block, 124, 12).trim() || "0", 8);
    const type = String.fromCharCode(block[156]);
    const prefix = readField(block, 345, 155);
    const data = tarBuffer.subarray(offset, offset + size);
    offset += Math.ceil(size / 512) * 512;

    if (type === "L") {
      pendingName = data.toString("utf8").replace(/\0+$/, "");
      continue;
    }
    const fullName = pendingName ?? (prefix ? `${prefix}/${name}` : name);
    pendingName = null;
    if (type === "0" || type === "\0" || type === "") {
      files.set(fullName.replace(/\0+$/, ""), data);
    }
  }
  return files;
}

function writeSourceTree(files, root) {
  const normalizedRoot = resolve(root) + sep;
  for (const [archivePath, contents] of files) {
    if (!archivePath.startsWith(ARCHIVE_PREFIX)) continue;
    const relative = archivePath.slice(ARCHIVE_PREFIX.length);
    if (!relative) continue;
    const output = resolve(root, relative);
    if (!output.startsWith(normalizedRoot)) {
      fail(`unsafe source archive path: ${archivePath}`);
    }
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(output, contents);
  }
}

async function main() {
  if (isExpectedSidecar(destination)) {
    console.log(`Antigravity sidecar up to date: ${destination}`);
    return;
  }

  const buildRoot = mkdtempSync(join(tmpdir(), "cam-ccusage-antigravity-"));
  try {
    const [sourceTgz, pricing] = await Promise.all([
      downloadVerified(
        ARCHIVE_URL,
        ARCHIVE_SHA512,
        `ccusage PR #${PR_NUMBER} commit ${COMMIT}`,
      ),
      downloadVerified(PRICING_URL, PRICING_SHA512, "pinned LiteLLM pricing"),
    ]);
    writeSourceTree(extractTar(gunzipSync(sourceTgz)), buildRoot);
    const pricingPath = join(buildRoot, "litellm-pricing.json");
    writeFileSync(pricingPath, pricing);

    const result = spawnSync(
      process.env.CARGO ?? "cargo",
      ["build", "--release", "-p", "ccusage", "--locked"],
      {
        cwd: join(buildRoot, "rust"),
        env: {
          ...process.env,
          CCUSAGE_VERSION: VERSION,
          CCUSAGE_PRICING_JSON_PATH: pricingPath,
        },
        stdio: "inherit",
      },
    );
    if (result.error) fail(`failed to start Cargo: ${result.error.message}`);
    if (result.status !== 0) fail(`Cargo exited with status ${result.status}`);

    const builtPath = join(buildRoot, "rust", "target", "release", "ccusage.exe");
    if (!existsSync(builtPath)) fail(`Cargo did not produce ${builtPath}`);
    const executable = readFileSync(builtPath);
    if (!isExpectedSidecar(builtPath)) {
      fail("built executable has the wrong version or lacks the Antigravity command");
    }
    const executableSha256 = digest(executable, "sha256");

    mkdirSync(dirname(destination), { recursive: true });
    const temporary = `${destination}.tmp`;
    writeFileSync(temporary, executable);
    if (existsSync(destination)) rmSync(destination);
    renameSync(temporary, destination);
    console.log(
      `staged Antigravity sidecar: ${destination} (${COMMIT}, sha256 ${executableSha256})`,
    );
  } finally {
    rmSync(buildRoot, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});
