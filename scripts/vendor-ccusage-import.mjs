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
//   4. apply the CAM downstream patch series (patches/0002-*.patch) with
//      `git apply` — this reproduces the split-architecture antigravity
//      port, the offline pricing build.rs fallback, the additive
//      models.dev pricing entry and the in-process PoC seam;
//   5. fetch the pinned LiteLLM pricing snapshot (SHA-256 verified);
//   6. copy the committed PATCHES.md (never regenerated here) and write
//      UPSTREAM.toml / pricing-manifest.json / MANIFEST.sha256;
//   7. byte-compare every staged file against the committed vendor blobs —
//      any mismatch aborts before the swap;
//   8. swap staging into the vendor tree (old tree backed up and restored
//      on failure).
//
// Preconditions enforced by this script:
//   - the committed vendor tree is git-clean (no uncommitted vendor edits);
//   - patches/0002-cam-downstream-v20.0.20.patch applies cleanly to the
//     pristine export. If vendor edits were made without regenerating that
//     patch, step 7 fails: regenerate the patch first (see PATCHES.md
//     "Regression risk & upgrade path").
//
// Usage: node scripts/vendor-ccusage-import.mjs [--work-dir <scratch dir>]

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

// The CAM downstream patch series applied on top of the pristine export, in
// order. 0001 is the verbatim upstream PR diff (audit reference only — it does
// not apply to the v20.0.20 split architecture). 0002 is the generated,
// apply-able representation of every committed downstream edit.
const CAM_PATCHES = ["0002-cam-downstream-v20.0.20.patch"];
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
	// Generator-owned files whose content is fully determined by this script.
	const GENERATED = new Set(["UPSTREAM.toml", "pricing/pricing-manifest.json"]);

	console.log(`work dir: ${workDir}`);

	// 0. Preconditions: the committed vendor tree must be clean so the swap
	// below cannot discard uncommitted vendor edits.
	const dirty = git(repoRoot, "status", "--porcelain", "--", "src-tauri/vendor/ccusage").trim();
	if (dirty) {
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
		execFileSync("git", ["apply", "--check", path.join("patches", patchFile)], {
			cwd: staging,
			maxBuffer: 64 * 1024 * 1024,
		});
		execFileSync("git", ["apply", path.join("patches", patchFile)], {
			cwd: staging,
			maxBuffer: 64 * 1024 * 1024,
		});
		// Keep the applied patch in the staged patches/ dir so the committed
		// set matches what the rebuild used.
		fs.copyFileSync(repoPatch, path.join(staging, "patches", patchFile));
	}
	for (const patchFile of REFERENCE_PATCHES) {
		if (!fs.existsSync(path.join(vendorRoot, "patches", patchFile))) {
			throw new Error(`reference patch missing from the repository: ${patchFile}`);
		}
		fs.copyFileSync(
			path.join(vendorRoot, "patches", patchFile),
			path.join(staging, "patches", patchFile),
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
	fs.writeFileSync(path.join(staging, "pricing", "litellm-pricing.json"), pricingBuffer);

	// 5. Preserve the committed PATCHES.md (hand-written; never regenerated).
	const patchesDoc = path.join(vendorRoot, "PATCHES.md");
	if (!fs.existsSync(patchesDoc)) {
		throw new Error("vendor/ccusage/PATCHES.md is missing; it is not generated by this script");
	}
	fs.copyFileSync(patchesDoc, path.join(staging, "PATCHES.md"));

	// 6. Auditable manifests. models-dev digest is taken from the post-patch
	// snapshot (pristine upstream + the one additive entry from PR #1487).
	const modelsDevPath = path.join(staging, "rust", "crates", "ccusage-core", "src", "models-dev-pricing.json");
	const modelsDevBuffer = fs.readFileSync(modelsDevPath);
	const modelsDevSha256 = sha256(modelsDevBuffer);
	const modelsDevEntries = Object.keys(JSON.parse(modelsDevBuffer.toString("utf8"))).length;

	fs.writeFileSync(
		path.join(staging, "pricing", "pricing-manifest.json"),
		`${JSON.stringify(
			{
				litellm: {
					source: litellmUrl,
					commit: BASELINE.litellm.commit,
					file: BASELINE.litellm.file,
					sha256: BASELINE.litellm.sha256,
					bytes: pricingBuffer.length,
					license: BASELINE.litellm.license,
				},
				modelsDev: {
					source: "ccusage v20.0.20 vendored snapshot (rust/crates/ccusage-core/src/models-dev-pricing.json)",
					upstream: `${BASELINE.upstream.repo}/blob/${BASELINE.upstream.commit}/rust/crates/ccusage-core/src/models-dev-pricing.json`,
					entries: modelsDevEntries,
					sha256: modelsDevSha256,
					sha256_upstream_pristine: pristineModelsDevSha256,
					divergence:
						"0002 patch: additive merge of the antigravity alias entry \"gemini-3.1-pro\" from the PR #1487 snapshot (fork c58c1b3aab2eacc82add250c8229bb6192e4489b); everything else is the pristine upstream blob. Antigravity model ids gemini-3.5-flash-high/-medium/-extra-low, gpt-oss-120b-medium and gemini-3-flash-a/b/c have no entry in either snapshot and intentionally stay unpriced (null cost), see PATCHES.md.",
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
note = "closed, unmerged downstream patch; ported manually onto the split-adapter v20.0.20 architecture; the apply-able representation of every committed downstream edit is patches/0002-cam-downstream-v20.0.20.patch (see PATCHES.md)"

[litellm_pricing]
repo = "${BASELINE.litellm.repo}"
commit = "${BASELINE.litellm.commit}"
file = "${BASELINE.litellm.file}"
sha256 = "${BASELINE.litellm.sha256}"

[models_dev_pricing]
# Embedded pricing snapshot consumed by ccusage-core (src/models-dev-pricing.json).
# See PATCHES.md 0001 for the one additive entry and pricing/pricing-manifest.json.
upstream = "${BASELINE.upstream.repo}/blob/${BASELINE.upstream.commit}/rust/crates/ccusage-core/src/models-dev-pricing.json"
entries = ${modelsDevEntries}
sha256 = "${modelsDevSha256}"
sha256_upstream_pristine = "${pristineModelsDevSha256}"
divergence = "0002 patch: additive merge of 'gemini-3.1-pro' from PR #1487 fork c58c1b3aab2eacc82add250c8229bb6192e4489b; all other entries pristine upstream"

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
				fs.renameSync(live, path.join(backup, entry));
				movedToBackup.push(entry);
			}
		}
		for (const entry of swapEntries) {
			fs.renameSync(path.join(staging, entry), path.join(vendorRoot, entry));
		}
		fs.copyFileSync(path.join(staging, "MANIFEST.sha256"), path.join(vendorRoot, "MANIFEST.sha256"));
	} catch (error) {
		// Roll back: restore whatever was moved out, remove half-swapped staging.
		for (const entry of swapEntries) {
			const live = path.join(vendorRoot, entry);
			fs.rmSync(live, { recursive: true, force: true });
			if (movedToBackup.includes(entry)) {
				fs.renameSync(path.join(backup, entry), live);
			}
		}
		throw new Error(`swap failed; vendor tree restored from backup. Cause: ${error.message}`);
	}

	console.log(`vendored ccusage baseline rebuilt and verified against the committed tree (${stagedRel.length + 1} files)`);
	console.log("next: cargo test --workspace in src-tauri/vendor/ccusage/rust, then pnpm vendor:verify");
	console.log("review the git diff of src-tauri/vendor/ccusage before committing.");
}

main();
