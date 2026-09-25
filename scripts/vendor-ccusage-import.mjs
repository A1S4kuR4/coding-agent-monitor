// Rebuild the vendored ccusage tree (src-tauri/vendor/ccusage) from the pinned
// upstream baselines and the CAM downstream patch series.
//
// This is the ONLY step that talks to the network for the vendored sources.
// It is meant to run explicitly during a vendor upgrade (see
// docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md section 11), never as part of
// `cargo build` / `pnpm tauri build`. Everything it fetches is pinned to an
// immutable commit SHA and verified before it lands in the vendor tree.
//
// Rebuild pipeline (all inside a scratch work dir; the live vendor tree is
// only touched after every check below has passed):
//   1. shallow-fetch the pinned ccusage v20.0.20 commit and the PR #1487 fork
//      commit, verify commit+tree identity;
//   2. export the pristine v20.0.20 rust subset into the staging tree;
//   3. regenerate the reference PR diff (patches/0001-*.patch, audit-only,
//      NOT applied);
//   4. apply the CAM downstream patch series (0002 and 0003) with
//      `git apply` — this reproduces the split-architecture antigravity
//      port, the offline pricing build.rs fallback, the additive
//      models.dev pricing entry, the in-process PoC seam, and the pricing
//      refresh's fixed-rate test expectation;
//   5. fetch pinned pricing refreshes and merge them over the v20.0.20
//      snapshots, retaining keys removed upstream for historical records;
//   6. copy the committed PATCHES.md (never regenerated here) and write
//      UPSTREAM.toml / pricing-manifest.json / MANIFEST.sha256;
//   7. byte-compare every staged file against the committed vendor blobs —
//      any mismatch aborts before the swap;
//   8. swap staging into the vendor tree (old tree backed up and restored
//      on failure).
//
// Preconditions enforced by this script:
//   - the committed vendor tree is git-clean (no uncommitted vendor edits);
//   - patches/0002 and 0003 apply cleanly to the
//     pristine export. If vendor edits were made without regenerating that
//     patch, step 7 fails: regenerate the patch first (see PATCHES.md
//     "Regression risk & upgrade path").
//
// Usage: node scripts/vendor-ccusage-import.mjs [--work-dir <scratch dir>]
//        node scripts/vendor-ccusage-import.mjs --check-staged

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const vendorRoot = path.join(repoRoot, "src-tauri", "vendor", "ccusage");

// Immutable upstream baselines. Must match docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md.
const BASELINE = {
	upstream: {
		repo: "https://github.com/ccusage/ccusage",
		tag: "v20.0.20",
		commit: "bd7f89b469aee5635fb2e6722dd6d70f2d113ac1",
		tree: "0acb7f0e9451a3094739a0caff0875ad035432e5",
	},
	antigravityPatch: {
		pr: "https://github.com/ccusage/ccusage/pull/1487",
		fork: "https://github.com/sambitcreate/ccusage",
		commit: "c58c1b3aab2eacc82add250c8229bb6192e4489b",
		tree: "c489445deffa68bcf863ed97ca29dce948c40509",
		baseCommit: "739e88fa67b9e584dfa9722c8207fa8b09b62802",
	},
	litellm: {
		repo: "https://github.com/BerriAI/litellm",
		commit: "1a183efaa1a2108aed7e1bed8d445d93bd1aa60d",
		file: "model_prices_and_context_window.json",
		sha256: "a74538d2edc13e1eb4f67870fbc2ee05035326e6eaed0dc5bce11d372cff6e60",
		license: "MIT (per repository licensing outside the enterprise directory), copyright Berri AI",
	},
	// Date of the original v20.0.20 import. Deliberately fixed per baseline (not
	// stamped at run time) so a rebuild reproduces the committed UPSTREAM.toml
	// byte-for-byte; bump it only when the baseline itself moves.
	importedAt: "2026-08-29",
};

// Pricing is refreshed independently of the parser version. The raw inputs
// are immutable GitHub blobs; their hashes are checked before any merge.
const PRICING_REFRESH = {
	litellm: {
		commit: "2dccc0dc79143043889bfaf2a9ecb315e5b197e8",
		sha256: "29906a2b1e9eca5b591bc6b30013fb57e298677cf5c77f29144a7846928dc46d",
	},
	modelsDev: {
		commit: "60377f71a96b185be209ef8ad1d7725944a6486a",
		sha256: "37ea6f07834a43a88873bc22835f13b3fe53cd1c6fd51f383e0545a025d56173",
		rulesSha256: "57dff8900c025ae3b2e3583247b0f7babba47ba73536af6a643bf0cd0bc81365",
	},
};

function mergePricingSnapshot(previous, current, indentation) {
	const oldEntries = JSON.parse(previous.toString("utf8"));
	const currentText = current.toString("utf8").trimEnd();
	const newEntries = JSON.parse(currentText);
	if (!currentText.endsWith("}")) throw new Error("pricing refresh must be a JSON object");
	const legacyKeys = Object.keys(oldEntries)
		.filter((key) => !Object.hasOwn(newEntries, key))
		.sort();
	// Keep the upstream blob's formatting and entry order intact; only append
	// removed historical keys. This makes the actual rate changes reviewable.
	const prefix = currentText.slice(0, -1).trimEnd();
	const additions = legacyKeys.map((key) =>
		`${indentation}${JSON.stringify(key)}: ${JSON.stringify(oldEntries[key], null, indentation).replaceAll("\n", `\n${indentation}`)}`,
	);
	const mergedText = `${prefix}${additions.length ? `,\n${additions.join(",\n")}` : ""}\n}\n`;
	JSON.parse(mergedText);
	return {
		buffer: Buffer.from(mergedText),
		entries: Object.keys(newEntries).length + legacyKeys.length,
		retained: legacyKeys.length,
	};
}

// The CAM downstream patch series applied on top of the pristine export, in
// order. 0001 is the verbatim upstream PR diff (audit reference only — it does
// not apply to the v20.0.20 split architecture). 0002 is the baseline CAM
// patch; 0003 updates one price-sensitive test for the separately pinned data.
const CAM_PATCHES = ["0002-cam-downstream-v20.0.20.patch", "0003-pricing-refresh-test.patch"];
const REFERENCE_PATCHES = ["0001-antigravity-c58c1b3.patch"];

// v20.0.20 archive digests observed during the 2026-08-29 import. GitHub
// archive byte representations are not a long-term identity (the plan records
// them for download audit only); commit + tree SHA above are the identity.
const ARCHIVE_SHA512 = {
	upstream: "110fd685c7887a9623ba528ede741307c94aca69e4fde42a1bd3e7eee4d8eecb44bc3064f750a45aa7537d5893ae49865dbc7c54c19059465cb389667ac60a4e",
	patchHead: "f42cdf6ac8e9f375aa0cccfd97e0019166d4b331f71b8f02bb206bd4f038b7dda43b593c81488a5d0f2ae8677a32092c276f31f3308c72c87374a948648ec57e",
};

// Rust sources required for unified daily collection. Everything else in the
// upstream workspace (ccusage CLI bin, cli-parser, config, npm launcher,
// docs site, benchmarks) is deliberately not vendored.
const EXPORT_PATHS = [
	"rust/Cargo.toml",
	"rust/Cargo.lock",
	"rust/adapters/README.md",
	"rust/adapters/amp",
	"rust/adapters/claude",
	"rust/adapters/codebuff",
	"rust/adapters/codex",
	"rust/adapters/common",
	"rust/adapters/copilot",
	"rust/adapters/droid",
	"rust/adapters/gemini",
	"rust/adapters/goose",
	"rust/adapters/grok",
	"rust/adapters/hermes",
	"rust/adapters/kilo",
	"rust/adapters/kimi",
	"rust/adapters/openclaw",
	"rust/adapters/opencode",
	"rust/adapters/pi",
	"rust/adapters/qwen",
	"rust/crates/ccusage-adapter-all",
	"rust/crates/ccusage-cli",
	"rust/crates/ccusage-core",
	"rust/crates/ccusage-terminal",
	"rust/crates/ccusage-test-support",
];

function git(cwd, ...args) {
	return execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
}

function sha512(buffer) {
	return createHash("sha512").update(buffer).digest("hex");
}

function sha256(buffer) {
	return createHash("sha256").update(buffer).digest("hex");
}

function fetchCommit(workDir, repoUrl, commit, expectedTree, extraCommits = []) {
	const dir = path.join(workDir, commit.slice(0, 12));
	fs.mkdirSync(dir, { recursive: true });
	git(dir, "init", "--quiet");
	const remotes = git(dir, "remote").trim().split("\n").filter(Boolean);
	if (remotes.includes("origin")) {
		git(dir, "remote", "set-url", "origin", repoUrl);
	} else {
		git(dir, "remote", "add", "origin", repoUrl);
	}
	git(dir, "-c", "core.autocrlf=false", "fetch", "--quiet", "--depth=1", "origin", commit);
	for (const extra of extraCommits) {
		git(dir, "-c", "core.autocrlf=false", "fetch", "--quiet", "--depth=1", "origin", extra);
	}
	git(dir, "-c", "core.autocrlf=false", "checkout", "--quiet", commit);
	const [commitId, treeId] = git(dir, "show", "-s", "--format=%H %T", commit).trim().split(" ");
	if (commitId !== commit || treeId !== expectedTree) {
		throw new Error(
			`upstream identity mismatch for ${repoUrl}: got commit ${commitId} tree ${treeId}, expected ${commit} tree ${expectedTree}`,
		);
	}
	return dir;
}

function walkFiles(root) {
	const out = [];
	const visit = (dir) => {
		for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
			const full = path.join(dir, entry.name);
			if (entry.isDirectory()) visit(full);
			else out.push(full);
		}
	};
	visit(root);
	return out.sort();
}

function readCommittedBlobs() {
	// Committed content of every tracked file under the vendor root: the
	// rebuild target. Read from git objects (not the working tree) so the
	// comparison is independent of checkout line-ending smudging.
	const ls = git(repoRoot, "-c", "core.autocrlf=false", "ls-files", "-s", "--", "src-tauri/vendor/ccusage");
	const blobs = new Map();
	for (const line of ls.split("\n")) {
		if (!line.trim()) continue;
		// `<mode> <blob hash> <stage>\t<path>` — split metadata from path on the tab.
		const tab = line.indexOf("\t");
		const [, hash] = line.slice(0, tab).trim().split(/\s+/);
		const rel = path.relative(vendorRoot, path.join(repoRoot, line.slice(tab + 1).trim()));
		const content = execFileSync("git", ["-c", "core.autocrlf=false", "cat-file", "blob", hash], {
			cwd: repoRoot,
			maxBuffer: 256 * 1024 * 1024,
		});
		blobs.set(rel.split(path.sep).join("/"), content);
	}
	return blobs;
}

/// Raw committed content of one tracked file, straight from git objects —
/// immune to checkout line-ending smudging (on Windows, `* text=auto` checks
/// text files out as CRLF, which would corrupt a byte-compare against the
/// LF committed blobs when copying files from the working tree).
/// Move a directory tree, falling back to copy+delete when `rename` fails
/// with EXDEV (source and target on different drives — the default work dir
/// lives in the OS temp dir, which may be a different device than the repo).
function moveDir(from, to) {
	try {
		fs.renameSync(from, to);
		return;
	} catch (error) {
		if (error.code !== "EXDEV") throw error;
	}
	fs.cpSync(from, to, { recursive: true });
	fs.rmSync(from, { recursive: true, force: true });
}

function gitBlob(repoPath) {
	return execFileSync("git", ["-c", "core.autocrlf=false", "show", `:${repoPath}`], {
		cwd: repoRoot,
		maxBuffer: 64 * 1024 * 1024,
	});
}

function main() {
	const argv = process.argv.slice(2);
	const workDirFlag = argv.indexOf("--work-dir");
	const workDir =
		workDirFlag >= 0 && argv[workDirFlag + 1]
			? path.resolve(argv[workDirFlag + 1])
			: fs.mkdtempSync(path.join(os.tmpdir(), "ccusage-vendor-"));
	// --adopt-generated: first rebuild after the patch-flow bootstrap. The two
	// generator-owned manifests (UPSTREAM.toml, pricing-manifest.json) are
	// downgraded from strict byte-compare to "reported and adopted"; every
	// other file still must match the committed blobs exactly. Later rebuilds
	// must run WITHOUT this flag.
	const adoptGenerated = argv.includes("--adopt-generated");
	// Validate a prospective refresh before committing it. All vendor files
	// must be staged; this mode compares to index blobs and never swaps files.
	const checkStaged = argv.includes("--check-staged");
	// Generator-owned files whose content is fully determined by this script.
	const GENERATED = new Set(["UPSTREAM.toml", "pricing/pricing-manifest.json"]);

	console.log(`work dir: ${workDir}`);

	// 0. Preconditions: the committed vendor tree must be clean so the swap
	// below cannot discard uncommitted vendor edits.
	const dirty = git(repoRoot, "status", "--porcelain", "--", "src-tauri/vendor/ccusage").trim();
	if (dirty && !checkStaged) {
		throw new Error(
			`vendor tree has uncommitted changes; commit or revert them before rebuilding:\n${dirty}`,
		);
	}
	const committed = readCommittedBlobs();

	const upstreamDir = fetchCommit(workDir, BASELINE.upstream.repo, BASELINE.upstream.commit, BASELINE.upstream.tree);
	const patchDir = fetchCommit(
		workDir,
		BASELINE.antigravityPatch.fork,
		BASELINE.antigravityPatch.commit,
		BASELINE.antigravityPatch.tree,
		// The PR base commit is needed to produce the reference diff.
		[BASELINE.antigravityPatch.baseCommit],
	);

	// Staging tree — everything is built here first.
	const staging = path.join(workDir, "staging");
	fs.rmSync(staging, { recursive: true, force: true });
	fs.mkdirSync(path.join(staging, "rust"), { recursive: true });
	fs.mkdirSync(path.join(staging, "patches"), { recursive: true });
	fs.mkdirSync(path.join(staging, "pricing"), { recursive: true });
	// Give git apply an isolated root even when staging is created inside the
	// repository, and avoid a separate GNU patch dependency on Windows.
	git(staging, "init", "--quiet");
	git(staging, "config", "core.autocrlf", "false");

	// 1. Export the pristine v20.0.20 subset into staging/rust.
	// core.autocrlf=false keeps blob bytes (LF) intact: the vendor tree must
	// match the upstream git blobs so recorded digests stay meaningful.
	const tar = execFileSync(
		"git",
		["-c", "core.autocrlf=false", "archive", "HEAD", "--format=tar", ...EXPORT_PATHS],
		{
			cwd: upstreamDir,
			maxBuffer: 256 * 1024 * 1024,
		},
	);
	// Extract with tar into a staging dir first, then move: git archive emits
	// `rust/...` prefixed paths, and we want the staging root to *be* `rust/`.
	const stage = path.join(workDir, "stage");
	fs.rmSync(stage, { recursive: true, force: true });
	fs.mkdirSync(stage, { recursive: true });
	fs.writeFileSync(path.join(stage, "export.tar"), tar);
	execFileSync("tar", ["-xf", "export.tar"], { cwd: stage });
	for (const entry of fs.readdirSync(path.join(stage, "rust"))) {
		fs.renameSync(path.join(stage, "rust", entry), path.join(staging, "rust", entry));
	}

	// LICENSE (the repo root LICENSE is a symlink to apps/ccusage/LICENSE).
	fs.writeFileSync(
		path.join(staging, "LICENSE"),
		execFileSync("git", ["show", `HEAD:apps/ccusage/LICENSE`], { cwd: upstreamDir }),
	);

	// Pristine models.dev snapshot digest, recorded before the CAM patch adds
	// its one additive entry (audit reference for the divergence fields below).
	const pristineModelsDevSha256 = sha256(
		fs.readFileSync(path.join(staging, "rust", "crates", "ccusage-core", "src", "models-dev-pricing.json")),
	);

	// 2. Record PR #1487 as a reference patch (base..head, verbatim upstream).
	// Audit reference only: it was written against the v20.0.18 monolithic
	// crate and does NOT apply to the v20.0.20 split architecture.
	const prDiff = git(patchDir, "diff", BASELINE.antigravityPatch.baseCommit, BASELINE.antigravityPatch.commit);
	fs.writeFileSync(path.join(staging, "patches", "0001-antigravity-c58c1b3.patch"), prDiff);

	// 3. Apply the CAM downstream patch series to the pristine export.
	for (const patchFile of CAM_PATCHES) {
		const repoPatch = path.join(vendorRoot, "patches", patchFile);
		if (!fs.existsSync(repoPatch)) {
			throw new Error(`CAM patch missing from the repository: ${repoPatch}`);
		}
		// Apply inside the isolated staging repository. A failed hunk aborts.
		// Copy from the staged blob, not the working tree, so a CRLF
		// checkout cannot leak into the staged bytes or the applied result.
		fs.writeFileSync(path.join(staging, "patches", patchFile), gitBlob(`src-tauri/vendor/ccusage/patches/${patchFile}`));
		execFileSync("git", ["apply", "--unsafe-paths", "--whitespace=nowarn", path.join("patches", patchFile)], {
			cwd: staging,
			maxBuffer: 64 * 1024 * 1024,
		});
	}
	fs.rmSync(path.join(staging, ".git"), { recursive: true, force: true });
	for (const patchFile of REFERENCE_PATCHES) {
		if (!fs.existsSync(path.join(vendorRoot, "patches", patchFile))) {
			throw new Error(`reference patch missing from the repository: ${patchFile}`);
		}
		// Staged blob, not the working tree (see the CRLF note above).
		fs.writeFileSync(
			path.join(staging, "patches", patchFile),
			gitBlob(`src-tauri/vendor/ccusage/patches/${patchFile}`),
		);
	}

	// 4. LiteLLM pricing snapshot at the pinned commit.
	const litellmUrl = `${BASELINE.litellm.repo}/raw/${BASELINE.litellm.commit}/${BASELINE.litellm.file}`;
	const pricingBuffer = execFileSync("curl", ["-sL", "-o", "-", litellmUrl], { maxBuffer: 64 * 1024 * 1024 });
	if (sha256(pricingBuffer) !== BASELINE.litellm.sha256) {
		throw new Error(
			`LiteLLM pricing SHA-256 mismatch: got ${sha256(pricingBuffer)}, expected ${BASELINE.litellm.sha256}`,
		);
	}
	const litellmRefreshUrl = `${BASELINE.litellm.repo}/raw/${PRICING_REFRESH.litellm.commit}/${BASELINE.litellm.file}`;
	const litellmRefreshBuffer = execFileSync("curl", ["-sL", "-o", "-", litellmRefreshUrl], { maxBuffer: 64 * 1024 * 1024 });
	if (sha256(litellmRefreshBuffer) !== PRICING_REFRESH.litellm.sha256) {
		throw new Error("refreshed LiteLLM pricing SHA-256 mismatch");
	}
	const mergedLitellm = mergePricingSnapshot(pricingBuffer, litellmRefreshBuffer, "    ");
	fs.writeFileSync(path.join(staging, "pricing", "litellm-pricing.json"), mergedLitellm.buffer);

	const modelsDevRefreshBase = `${BASELINE.upstream.repo}/raw/${PRICING_REFRESH.modelsDev.commit}/rust/crates/ccusage-core/src`;
	const modelsDevRefreshUrl = `${modelsDevRefreshBase}/models-dev-pricing.json`;
	const modelsDevRefreshBuffer = execFileSync("curl", ["-sL", "-o", "-", modelsDevRefreshUrl], { maxBuffer: 64 * 1024 * 1024 });
	if (sha256(modelsDevRefreshBuffer) !== PRICING_REFRESH.modelsDev.sha256) {
		throw new Error("refreshed models.dev pricing SHA-256 mismatch");
	}
	const rulesBuffer = execFileSync("curl", ["-sL", "-o", "-", `${modelsDevRefreshBase}/models-dev-catalog-rules.json`], { maxBuffer: 64 * 1024 * 1024 });
	if (sha256(rulesBuffer) !== PRICING_REFRESH.modelsDev.rulesSha256) {
		throw new Error("refreshed models.dev catalog rules SHA-256 mismatch");
	}
	const modelsDevPath = path.join(staging, "rust", "crates", "ccusage-core", "src", "models-dev-pricing.json");
	const mergedModelsDev = mergePricingSnapshot(fs.readFileSync(modelsDevPath), modelsDevRefreshBuffer, "\t");
	fs.writeFileSync(modelsDevPath, mergedModelsDev.buffer);
	fs.writeFileSync(path.join(staging, "rust", "crates", "ccusage-core", "src", "models-dev-catalog-rules.json"), rulesBuffer);

	// 5. Preserve the committed PATCHES.md (hand-written; never regenerated).
	const patchesDoc = path.join(vendorRoot, "PATCHES.md");
	if (!fs.existsSync(patchesDoc)) {
		throw new Error("vendor/ccusage/PATCHES.md is missing; it is not generated by this script");
	}
	fs.copyFileSync(patchesDoc, path.join(staging, "PATCHES.md"));

	// 6. Auditable manifests. The v20.0.20 baseline remains identifiable;
	// price refreshes are separately pinned and merged deterministically.
	const modelsDevBuffer = fs.readFileSync(modelsDevPath);
	const modelsDevSha256 = sha256(modelsDevBuffer);
	const modelsDevEntries = Object.keys(JSON.parse(modelsDevBuffer.toString("utf8"))).length;

	fs.writeFileSync(
		path.join(staging, "pricing", "pricing-manifest.json"),
		`${JSON.stringify(
			{
				litellm: {
					source: litellmRefreshUrl,
					commit: PRICING_REFRESH.litellm.commit,
					file: BASELINE.litellm.file,
					sha256: sha256(mergedLitellm.buffer),
					bytes: mergedLitellm.buffer.length,
					sourceSha256: PRICING_REFRESH.litellm.sha256,
					baselineCommit: BASELINE.litellm.commit,
					baselineSha256: BASELINE.litellm.sha256,
					retainedLegacyKeys: mergedLitellm.retained,
					license: BASELINE.litellm.license,
				},
				modelsDev: {
					source: "ccusage v20.0.20 snapshot with pinned pricing refresh",
					upstream: modelsDevRefreshUrl,
					commit: PRICING_REFRESH.modelsDev.commit,
					sourceSha256: PRICING_REFRESH.modelsDev.sha256,
					rulesSha256: PRICING_REFRESH.modelsDev.rulesSha256,
					entries: modelsDevEntries,
					sha256: modelsDevSha256,
					sha256_upstream_pristine: pristineModelsDevSha256,
					retainedLegacyKeys: mergedModelsDev.retained,
					divergence:
						"Pinned ccusage pricing refresh takes precedence; v20.0.20 keys absent from the refresh are retained for historical logs, including the Antigravity gemini-3.1-pro alias from patch 0002. See PATCHES.md.",
				},
			},
			null,
			2,
		)}\n`,
	);

	const upstreamToml = `# Upstream provenance for the vendored ccusage Rust sources.
# Generated by scripts/vendor-ccusage-import.mjs; do not edit by hand.

[upstream]
repo = "${BASELINE.upstream.repo}"
tag = "${BASELINE.upstream.tag}"
commit = "${BASELINE.upstream.commit}"
tree = "${BASELINE.upstream.tree}"
license = "MIT"
source_url = "${BASELINE.upstream.repo}/tree/${BASELINE.upstream.commit}"

[antigravity_patch]
pr = "${BASELINE.antigravityPatch.pr}"
fork = "${BASELINE.antigravityPatch.fork}"
commit = "${BASELINE.antigravityPatch.commit}"
tree = "${BASELINE.antigravityPatch.tree}"
base_commit = "${BASELINE.antigravityPatch.baseCommit}"
patch_file = "patches/0001-antigravity-c58c1b3.patch"
license = "MIT"
note = "closed, unmerged downstream patch; ported manually onto v20.0.20 by 0002; pricing refresh and its test patch 0003 are recorded in PATCHES.md"

[litellm_pricing]
repo = "${BASELINE.litellm.repo}"
commit = "${PRICING_REFRESH.litellm.commit}"
file = "${BASELINE.litellm.file}"
sha256 = "${sha256(mergedLitellm.buffer)}"
source_sha256 = "${PRICING_REFRESH.litellm.sha256}"
baseline_commit = "${BASELINE.litellm.commit}"
baseline_sha256 = "${BASELINE.litellm.sha256}"
retained_legacy_keys = ${mergedLitellm.retained}

[models_dev_pricing]
# Embedded pricing snapshot consumed by ccusage-core (src/models-dev-pricing.json).
# The refreshed ccusage pricing table takes precedence; v20.0.20-only keys stay.
upstream = "${modelsDevRefreshUrl}"
commit = "${PRICING_REFRESH.modelsDev.commit}"
source_sha256 = "${PRICING_REFRESH.modelsDev.sha256}"
rules_sha256 = "${PRICING_REFRESH.modelsDev.rulesSha256}"
entries = ${modelsDevEntries}
sha256 = "${modelsDevSha256}"
sha256_upstream_pristine = "${pristineModelsDevSha256}"
retained_legacy_keys = ${mergedModelsDev.retained}
divergence = "Pinned ccusage pricing refresh takes precedence; v20.0.20-only keys remain for historical logs, including patch 0002's gemini-3.1-pro alias"

[archive_digests]
# SHA-512 of the GitHub codeload tar.gz archives observed during the import.
# Informational only: GitHub archive bytes are not a stable identity; the
# commit + tree SHAs above are. Verified against the plan document on import.
upstream_tar_gz = "${ARCHIVE_SHA512.upstream}"
patch_head_tar_gz = "${ARCHIVE_SHA512.patchHead}"

[import]
imported_at = "${BASELINE.importedAt}"
imported_by = "scripts/vendor-ccusage-import.mjs"
scope = "rust workspace subset required for unified daily collection (core, all agent adapters, unified adapter, pricing); CLI bin/parser/config, npm launcher, docs site and benchmarks excluded"
`;
	fs.writeFileSync(path.join(staging, "UPSTREAM.toml"), upstreamToml);

	// 7. Prove the rebuild reproduces the committed vendor tree byte-for-byte
	// BEFORE touching the live tree. Staged bytes must equal the committed
	// git blobs; the committed blob set must equal the staged file set.
	const stagedRel = walkFiles(staging)
		.map((file) => path.relative(staging, file).split(path.sep).join("/"))
		.sort();
	const expected = [...committed.keys()].filter((rel) => rel !== "MANIFEST.sha256").sort();
	const onlyStaged = stagedRel.filter((rel) => !expected.includes(rel));
	const onlyCommitted = expected.filter((rel) => !stagedRel.includes(rel));
	if (onlyStaged.length || onlyCommitted.length) {
		throw new Error(
			`rebuilt tree does not match the committed vendor file set.\n` +
				`only in rebuilt: ${JSON.stringify(onlyStaged)}\n` +
				`only in committed: ${JSON.stringify(onlyCommitted)}\n` +
				`Regenerate patches/0002-cam-downstream-v20.0.20.patch (see PATCHES.md) before re-running the import.`,
		);
	}
	const drifted = [];
	const adoptedGenerated = [];
	for (const rel of stagedRel) {
		const staged = fs.readFileSync(path.join(staging, ...rel.split("/")));
		if (sha256(staged) === sha256(committed.get(rel))) continue;
		if (GENERATED.has(rel)) {
			if (adoptGenerated) {
				adoptedGenerated.push(rel);
				continue;
			}
			throw new Error(
				`generator-owned manifest ${rel} differs from the committed version. ` +
					`If this rebuild intentionally updates the import flow, re-run with --adopt-generated; otherwise regenerate the committed manifests from the current script.`,
			);
		}
		drifted.push(rel);
	}
	if (drifted.length) {
		throw new Error(
			`rebuilt files differ from the committed vendor blobs:\n  ${drifted.join("\n  ")}\n` +
				`Regenerate patches/0002-cam-downstream-v20.0.20.patch (see PATCHES.md) before re-running the import.`,
		);
	}
	for (const rel of adoptedGenerated) {
		console.log(`adopted regenerated manifest: ${rel}`);
	}
	if (checkStaged) {
		console.log(`staged vendor tree reproduced byte-for-byte (${stagedRel.length} files); live files unchanged`);
		return;
	}

	// 8. MANIFEST.sha256 over the rebuilt tree (committed content identity,
	// independent of checkout line-ending smudging). Verified by
	// scripts/vendor-verify.mjs.
	const manifestLines = stagedRel.map((rel) => {
		const content = fs.readFileSync(path.join(staging, ...rel.split("/")));
		return `${sha256(content)}  ${rel}`;
	});
	fs.writeFileSync(path.join(staging, "MANIFEST.sha256"), `${manifestLines.join("\n")}\n`);

	// 9. Swap staging into the live vendor tree. Back up first; roll back on
	// any failure so the existing vendor is never left destroyed.
	const backup = path.join(workDir, "backup");
	fs.mkdirSync(backup, { recursive: true });
	const swapEntries = ["rust", "patches", "pricing", "LICENSE", "PATCHES.md", "UPSTREAM.toml"];
	const movedToBackup = [];
	try {
		for (const entry of swapEntries) {
			const live = path.join(vendorRoot, entry);
			if (fs.existsSync(live)) {
				moveDir(live, path.join(backup, entry));
				movedToBackup.push(entry);
			}
		}
		for (const entry of swapEntries) {
			moveDir(path.join(staging, entry), path.join(vendorRoot, entry));
		}
		fs.copyFileSync(path.join(staging, "MANIFEST.sha256"), path.join(vendorRoot, "MANIFEST.sha256"));
	} catch (error) {
		// Roll back: restore whatever was moved out, remove half-swapped staging.
		for (const entry of swapEntries) {
			const live = path.join(vendorRoot, entry);
			fs.rmSync(live, { recursive: true, force: true });
			if (movedToBackup.includes(entry)) {
				moveDir(path.join(backup, entry), live);
			}
		}
		throw new Error(`swap failed; vendor tree restored from backup. Cause: ${error.message}`);
	}

	console.log(`vendored ccusage baseline rebuilt and verified against the committed tree (${stagedRel.length + 1} files)`);
	console.log("next: cargo test --workspace in src-tauri/vendor/ccusage/rust, then pnpm vendor:verify");
	console.log("review the git diff of src-tauri/vendor/ccusage before committing.");
}

main();
