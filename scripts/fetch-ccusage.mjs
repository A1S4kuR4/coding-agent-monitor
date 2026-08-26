// Reproducibly stage the pinned ccusage native sidecar into `src-tauri/binaries/`.
//
// ccusage ships as an npm package whose optional platform binaries are pulled in
// transitively, but this application runs ccusage.exe as a Tauri external sidecar
// instead of as a project dependency. That means ccusage never appears in
// package.json. This script reproduces the exact Windows x64 native binary from
// the pinned @ccusage/ccusage-win32-x64 20.0.20 tarball on the npm registry:
//
//   1. download the pinned tarball URL
//   2. verify its SHA-512 integrity is identical to the registry `dist.integrity`
//   3. extract `package/bin/ccusage.exe` and verify its SHA-256 checksum
//   4. copy it to the Tauri target-triple filename used by `bundle.externalBin`
//
// It is idempotent: if the staged binary is already present and checks out, it
// does nothing and exits 0. Any mismatch (wrong version was published, a
// truncated download, a tampered tarball) fails the run with a clear message.

import { createHash } from "node:crypto";
import { gunzipSync } from "node:zlib";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Pinned sidecar source. Bump VERSION, re-derive the two hashes from the
// registry (dist.integrity) and from the extracted exe, and update them together.
const VERSION = "20.0.20";
const PLATFORM_PKG = "@ccusage/ccusage-win32-x64";
const TARGET_TRIPLE = "x86_64-pc-windows-msvc"; // Tauri target triple, Windows x64 MSVC.
// URL-encode the package scope for the path segment; the tarball filename uses
// the unscoped short name (the part after "/").
const SCOPE_PATH = PLATFORM_PKG.replace("/", "%2f");
const SHORT_NAME = PLATFORM_PKG.slice(PLATFORM_PKG.indexOf("/") + 1); // ccusage-win32-x64
const TGZ_FILENAME = `${SHORT_NAME}-${VERSION}.tgz`;
const TARBALL_URL = `https://registry.npmjs.org/${SCOPE_PATH}/-/${TGZ_FILENAME}`;

// sha512 base64 of the tarball as published, i.e. the registry `dist.integrity` value.
const TGZ_INTEGRITY_B64 =
  "6R9cdLuRy529ewdkKbwtjx7boCQNFK8KG1F6d5G/pw2ApKRmucWcwjFo/7sNULQtZMyUxhYa40tUnWy9L/yTMA==";
// sha256 (hex) of `package/bin/ccusage.exe` inside that tarball.
const EXE_SHA256 = "3fd2f3b9fba3a74881b9e75c76baa5574f99e19142394e855c5fbaaf22f245bc";

const ARCHIVE_EXE = "package/bin/ccusage.exe";

const here = dirname(fileURLToPath(import.meta.url));
const binariesDir = join(here, "..", "src-tauri", "binaries");
const destination = join(binariesDir, `ccusage-${TARGET_TRIPLE}.exe`);

function fail(message) {
  console.error(`[fetch-ccusage] error: ${message}`);
  process.exit(1);
}

function sha256Hex(buffer) {
  return createHash("sha256").update(buffer).digest("hex");
}

// Extract every regular file from a USTAR/pax-restricted tarball into a map of
// `path -> Buffer`. ccusage tarballs are plain USTAR with a `package/` prefix and
// stay well under the prefix-length limits, so an incremental USTAR parse (no
// external tar dependency) is all that is needed to stay reproducible anywhere
// Node runs. GNU long-name entries (type 'L') and pax headers are handled by
// remembering the last name forwarded to the following file entry.
function extractTar(tarBuffer) {
  const files = new Map();
  let offset = 0;
  // Read a field (name/size/prefix) from the current 512-byte `block`. Offsets
  // are block-relative; the block lives at tarBuffer[offset-512 .. offset].
  const readField = (block, start, len) => {
    const end = block.indexOf(0, start);
    const actualEnd = end === -1 || end > start + len ? start + len : end;
    return block.subarray(start, actualEnd).toString("utf8");
  };
  let pendingName = null;
  while (offset + 512 <= tarBuffer.length) {
    const block = tarBuffer.subarray(offset, offset + 512);
    offset += 512;
    if (block.every((byte) => byte === 0)) break; // two zero-blocks mark the end
    const name = readField(block, 0, 100);
    const size = parseInt(readField(block, 124, 12).trim() || "0", 8);
    const type = String.fromCharCode(block[156]); // '\0' => regular file
    const prefix = readField(block, 345, 155);
    const data = tarBuffer.subarray(offset, offset + size);
    offset += Math.ceil(size / 512) * 512;

    if (type === "L") {
      pendingName = data.toString("utf8").replace(/\0+$/, "");
      continue;
    }
    const fullName =
      pendingName ?? (prefix ? `${prefix}/${name}` : name.replace(/\0+$/, ""));
    pendingName = null;
    if (type === "0" || type === "\0" || type === "") {
      files.set(fullName, data);
    }
  }
  return files;
}

async function main() {
  // Idempotent fast path: staged binary already present with the right hash.
  if (existsSync(destination)) {
    const existing = readFileSync(destination);
    if (sha256Hex(existing) === EXE_SHA256) {
      console.log(`sidecar up to date: ${destination}`);
      return;
    }
    console.log(`staged binary hash mismatch; re-downloading...`);
  }

  const response = await fetch(TARBALL_URL);
  if (!response.ok) {
    fail(`failed to download ${TARBALL_URL}: HTTP ${response.status}`);
  }
  const tgz = Buffer.from(await response.arrayBuffer());

  const integrity =
    createHash("sha512").update(tgz).digest("base64");
  if (integrity !== TGZ_INTEGRITY_B64) {
    fail(
      `tarball SHA-512 integrity mismatch for ${TARBALL_URL}\n` +
        `  expected: ${TGZ_INTEGRITY_B64}\n` +
        `  actual:   ${integrity}`,
    );
  }

  const files = extractTar(gunzipSync(tgz));
  const exe = files.get(ARCHIVE_EXE);
  if (!exe) {
    fail(`"${ARCHIVE_EXE}" not found in ${PLATFORM_PKG}@${VERSION} tarball`);
  }
  if (sha256Hex(exe) !== EXE_SHA256) {
    fail(
      `ccusage.exe SHA-256 mismatch\n  expected: ${EXE_SHA256}\n  actual:   ${sha256Hex(exe)}`,
    );
  }

  mkdirSync(binariesDir, { recursive: true });
  const tmp = `${destination}.tmp`;
  writeFileSync(tmp, exe);
  if (existsSync(destination)) rmSync(destination);
  renameSync(tmp, destination);
  console.log(`staged sidecar: ${destination} (${exe.length} bytes)`);
}

main().catch((error) => fail(error?.stack ?? String(error)));