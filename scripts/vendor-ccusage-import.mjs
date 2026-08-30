// Import the pinned ccusage Rust source baseline into src-tauri/vendor/ccusage.
//
// This is the ONLY step that talks to the network for the vendored sources.
// It is meant to run explicitly during a vendor upgrade (see
// docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md section 11), never as part of
// `cargo build` / `pnpm tauri build`. Everything it fetches is pinned to an
// immutable commit SHA and verified before it lands in the vendor tree.
//
// Downstream modifications (the CAM patch series) are NOT applied by this
// script; they live as committed edits in the vendor tree and as patch files
// in vendor/ccusage/patches/. Re-running this script resets the vendored
// files to pristine upstream - the patch series must then be re-applied and
// re-reviewed.
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
	importedAt: new Date().toISOString().slice(0, 10),
};

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

function main() {
	const argv = process.argv.slice(2);
	const workDirFlag = argv.indexOf("--work-dir");
	const workDir =
		workDirFlag >= 0 && argv[workDirFlag + 1]
			? path.resolve(argv[workDirFlag + 1])
			: fs.mkdtempSync(path.join(os.tmpdir(), "ccusage-vendor-"));

	console.log(`work dir: ${workDir}`);
	const upstreamDir = fetchCommit(workDir, BASELINE.upstream.repo, BASELINE.upstream.commit, BASELINE.upstream.tree);
	const patchDir = fetchCommit(
		workDir,
		BASELINE.antigravityPatch.fork,
		BASELINE.antigravityPatch.commit,
		BASELINE.antigravityPatch.tree,
		// The PR base commit is needed to produce the reference diff.
		[BASELINE.antigravityPatch.baseCommit],
	);

	// Reset the vendor tree. Downstream-owned files (patches/, PATCHES.md,
	// pricing manifest) are preserved by being re-generated below.
	fs.rmSync(vendorRoot, { recursive: true, force: true });
	fs.mkdirSync(path.join(vendorRoot, "rust"), { recursive: true });
	fs.mkdirSync(path.join(vendorRoot, "patches"), { recursive: true });
	fs.mkdirSync(path.join(vendorRoot, "pricing"), { recursive: true });

	// 1. Export the pristine v20.0.20 subset into vendor/ccusage/rust.
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
	// `rust/...` prefixed paths, and we want the vendor root to *be* `rust/`.
	const stage = path.join(workDir, "stage");
	fs.rmSync(stage, { recursive: true, force: true });
	fs.mkdirSync(stage, { recursive: true });
	fs.writeFileSync(path.join(stage, "export.tar"), tar);
	execFileSync("tar", ["-xf", "export.tar"], { cwd: stage });
	fs.renameSync(path.join(stage, "rust", "Cargo.toml"), path.join(vendorRoot, "rust", "Cargo.toml"));
	fs.renameSync(path.join(stage, "rust", "Cargo.lock"), path.join(vendorRoot, "rust", "Cargo.lock"));
	for (const entry of fs.readdirSync(path.join(stage, "rust"))) {
		fs.renameSync(path.join(stage, "rust", entry), path.join(vendorRoot, "rust", entry));
	}

	// LICENSE (the repo root LICENSE is a symlink to apps/ccusage/LICENSE).
	fs.writeFileSync(
		path.join(vendorRoot, "LICENSE"),
		execFileSync("git", ["show", `HEAD:apps/ccusage/LICENSE`], { cwd: upstreamDir }),
	);

	// 2. Record PR #1487 as a reference patch (base..head, verbatim upstream).
	const prDiff = git(patchDir, "diff", BASELINE.antigravityPatch.baseCommit, BASELINE.antigravityPatch.commit);
	fs.writeFileSync(path.join(vendorRoot, "patches", "0001-antigravity-c58c1b3.patch"), prDiff);

	// 3. LiteLLM pricing snapshot at the pinned commit.
	const litellmUrl = `${BASELINE.litellm.repo}/raw/${BASELINE.litellm.commit}/${BASELINE.litellm.file}`;
	const pricingBuffer = execFileSync("curl", ["-sL", "-o", "-", litellmUrl], { maxBuffer: 64 * 1024 * 1024 });
	if (sha256(pricingBuffer) !== BASELINE.litellm.sha256) {
		throw new Error(
			`LiteLLM pricing SHA-256 mismatch: got ${sha256(pricingBuffer)}, expected ${BASELINE.litellm.sha256}`,
		);
	}
	fs.writeFileSync(path.join(vendorRoot, "pricing", "litellm-pricing.json"), pricingBuffer);

	// 4. Auditable manifests.
	fs.writeFileSync(
		path.join(vendorRoot, "pricing", "pricing-manifest.json"),
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
					source:
						"ccusage v20.0.20 vendored snapshot (rust/crates/ccusage-core/src/models-dev-pricing.json), replaced by the 0001 patch series entry with the PR #1487 Gemini-extended snapshot",
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
note = "closed, unmerged downstream patch; ported manually onto the split-adapter v20.0.20 architecture (see PATCHES.md)"

[litellm_pricing]
repo = "${BASELINE.litellm.repo}"
commit = "${BASELINE.litellm.commit}"
file = "${BASELINE.litellm.file}"
sha256 = "${BASELINE.litellm.sha256}"

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
	fs.writeFileSync(path.join(vendorRoot, "UPSTREAM.toml"), upstreamToml);

	console.log(`vendored ccusage baseline imported to ${vendorRoot}`);
	console.log("NOTE: downstream patches (see PATCHES.md) are NOT applied by the import;");
	console.log("      they are committed edits in this repository.");
}

main();
