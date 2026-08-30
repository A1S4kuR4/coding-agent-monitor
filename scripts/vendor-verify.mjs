// Read-only verification of the vendored ccusage tree (src-tauri/vendor/ccusage)
// — the `pnpm vendor:verify` gate for docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md
// Gate 0. Never writes, never fetches: every check runs offline.
//
// Checks:
//   1. upstream identity — UPSTREAM.toml tag/commit/tree must equal the pinned
//      baselines recorded here and in the v0.3 plan;
//   2. vendor manifest — MANIFEST.sha256 must cover exactly the git-tracked
//      vendor files, and every committed blob's SHA-256 must match it;
//   3. downstream patch state — the CAM patch files exist and the applied
//      markers (antigravity registration, in-process PoC seam, additive
//      models.dev entry) are present in the vendored sources;
//   4. pricing — LiteLLM snapshot SHA-256 and the models.dev snapshot SHA-256 /
//      entry count / additive entry match the recorded manifests;
//   5. offline pricing — the vendored-pricing build.rs fallback is present and
//      the network fetch feature (`fetch-litellm-pricing`) is not enabled for
//      the product build (minreq absent from the dependency graph);
//   6. SQLite linkage — libsqlite3-sys absent from the dependency graph, and
//      exactly one sqlite3-src implementation.
//
// Any mismatch exits non-zero.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const vendorRoot = path.join(repoRoot, "src-tauri", "vendor", "ccusage");
const srcTauriManifest = path.join(repoRoot, "src-tauri", "Cargo.toml");

// Pinned baselines — must match scripts/vendor-ccusage-import.mjs and
// docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md.
const EXPECTED = {
	upstream: {
		tag: "v20.0.20",
		commit: "bd7f89b469aee5635fb2e6722dd6d70f2d113ac1",
		tree: "0acb7f0e9451a3094739a0caff0875ad035432e5",
	},
	antigravityPatch: {
		commit: "c58c1b3aab2eacc82add250c8229bb6192e4489b",
		tree: "c489445deffa68bcf863ed97ca29dce948c40509",
		baseCommit: "739e88fa67b9e584dfa9722c8207fa8b09b62802",
	},
	litellm: {
		commit: "1a183efaa1a2108aed7e1bed8d445d93bd1aa60d",
		sha256: "a74538d2edc13e1eb4f67870fbc2ee05035326e6eaed0dc5bce11d372cff6e60",
	},
	modelsDev: {
		entries: 2275,
		// pristine v20.0.20 blob; the vendored snapshot adds one additive entry
		sha256_upstream_pristine: "be347bd498cb046c2045e018e068aa228a76b34485613e2254f21a48b889eecd",
	},
	additiveEntry: {
		name: "gemini-3.1-pro",
		cost: { cache_read: 0.2, cache_write: 0.375, input: 2, output: 12 },
		limit: { context: 1048576 },
	},
};

const failures = [];
function check(name, ok, details = "") {
	if (ok) {
		console.log(`PASS  ${name}`);
	} else {
		failures.push(name);
		console.error(`FAIL  ${name}${details ? ` — ${details}` : ""}`);
	}
}

function sha256(buffer) {
	return createHash("sha256").update(buffer).digest("hex");
}

function git(...args) {
	return execFileSync("git", args, { cwd: repoRoot, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
}

function parseTomlSection(text, section) {
	const match = text.match(new RegExp(`\\[${section}\\]\\s*([\\s\\S]*?)(?=\\n\\[|$)`));
	const fields = {};
	if (!match) return null;
	for (const line of match[1].split("\n")) {
		const m = line.match(/^([a-z_0-9]+)\s*=\s*"([^"]*)"$/) ?? line.match(/^([a-z_0-9]+)\s*=\s*(\d+)$/);
		if (m) fields[m[1]] = m[2];
	}
	return fields;
}

function cargoTree(...args) {
	return execFileSync(
		"cargo",
		["tree", "--offline", "--manifest-path", srcTauriManifest, ...args],
		{ encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
	);
}

function main() {
	// --- 1. upstream identity -------------------------------------------------
	const upstreamToml = fs.readFileSync(path.join(vendorRoot, "UPSTREAM.toml"), "utf8");
	const upstream = parseTomlSection(upstreamToml, "upstream");
	const patch = parseTomlSection(upstreamToml, "antigravity_patch");
	const litellm = parseTomlSection(upstreamToml, "litellm_pricing");
	const modelsDevToml = parseTomlSection(upstreamToml, "models_dev_pricing");

	check(
		"upstream identity (tag/commit/tree)",
		upstream?.tag === EXPECTED.upstream.tag &&
			upstream?.commit === EXPECTED.upstream.commit &&
			upstream?.tree === EXPECTED.upstream.tree,
	);
	check(
		"antigravity patch identity (commit/tree/base)",
		patch?.commit === EXPECTED.antigravityPatch.commit &&
			patch?.tree === EXPECTED.antigravityPatch.tree &&
			patch?.base_commit === EXPECTED.antigravityPatch.baseCommit,
	);
	check("litellm pin matches", litellm?.sha256 === EXPECTED.litellm.sha256 && litellm?.commit === EXPECTED.litellm.commit);
	check(
		"models.dev pin matches (pristine digest + entries)",
		modelsDevToml?.sha256_upstream_pristine === EXPECTED.modelsDev.sha256_upstream_pristine &&
			Number(modelsDevToml?.entries) === EXPECTED.modelsDev.entries,
	);

	// --- 2. vendor manifest ---------------------------------------------------
	const manifestPath = path.join(vendorRoot, "MANIFEST.sha256");
	check("MANIFEST.sha256 exists", fs.existsSync(manifestPath));
	const manifest = new Map();
	for (const line of fs.readFileSync(manifestPath, "utf8").split("\n")) {
		if (!line.trim()) continue;
		const [hash, rel] = line.trim().split(/\s{2}/);
		manifest.set(rel, hash);
	}

	const tracked = new Map(); // rel -> blob content
	const ls = git("-c", "core.autocrlf=false", "ls-files", "-s", "--", "src-tauri/vendor/ccusage");
	for (const line of ls.split("\n")) {
		if (!line.trim()) continue;
		const tab = line.indexOf("\t");
		const [, hash] = line.slice(0, tab).trim().split(/\s+/);
		const rel = path
			.relative(vendorRoot, path.join(repoRoot, line.slice(tab + 1).trim()))
			.split(path.sep)
			.join("/");
		tracked.set(rel, execFileSync("git", ["-c", "core.autocrlf=false", "cat-file", "blob", hash], { cwd: repoRoot, maxBuffer: 256 * 1024 * 1024 }));
	}

	const manifestOnly = [...manifest.keys()].filter((rel) => !tracked.has(rel));
	// MANIFEST.sha256 cannot cover itself; exclude it from the tracked set.
	const trackedOnly = [...tracked.keys()].filter((rel) => !manifest.has(rel) && rel !== "MANIFEST.sha256");
	check(
		"MANIFEST covers exactly the tracked vendor files",
		manifestOnly.length === 0 && trackedOnly.length === 0,
		`manifest-only: [${manifestOnly}] tracked-only: [${trackedOnly}]`,
	);
	const hashMismatches = [...tracked.keys()]
		.filter((rel) => rel !== "MANIFEST.sha256")
		.filter((rel) => sha256(tracked.get(rel)) !== manifest.get(rel));
	check(
		"every tracked vendor blob matches MANIFEST.sha256",
		hashMismatches.length === 0,
		`mismatched: [${hashMismatches.join(", ")}]`,
	);

	// --- 3. downstream patch state -------------------------------------------
	check(
		"patch files present (0001 reference, 0002 CAM)",
		manifest.has("patches/0001-antigravity-c58c1b3.patch") &&
			manifest.has("patches/0002-cam-downstream-v20.0.20.patch"),
	);
	const coreLib = tracked.get("rust/crates/ccusage-core/src/lib.rs")?.toString("utf8") ?? "";
	check(
		"antigravity registered in BUILT_IN_AGENT_NAMES",
		/"kilo", "copilot", "gemini", "kimi", "qwen", "grok", "antigravity",/.test(coreLib),
	);
	const adapterAllLib = tracked.get("rust/crates/ccusage-adapter-all/src/lib.rs")?.toString("utf8") ?? "";
	check(
		"in-process PoC seam present (daily_report_json_by_agent)",
		adapterAllLib.includes("pub fn daily_report_json_by_agent"),
	);
	const adapterAllLoader = tracked.get("rust/crates/ccusage-adapter-all/src/loader.rs")?.toString("utf8") ?? "";
	check(
		"antigravity loader registered in unified report",
		adapterAllLoader.includes("antigravity::load_entries") &&
			adapterAllLoader.includes('"Antigravity"'),
	);
	const adapterAllReport = tracked.get("rust/crates/ccusage-adapter-all/src/report.rs")?.toString("utf8") ?? "";
	check(
		"antigravity agent label mapped",
		adapterAllReport.includes(`"antigravity" => "Antigravity"`),
	);
	const antigravityLoader = tracked.get("rust/adapters/antigravity/src/loader.rs");
	check("antigravity adapter crate present", antigravityLoader !== undefined);

	// --- 4. pricing -----------------------------------------------------------
	const litellmBuffer = tracked.get("pricing/litellm-pricing.json");
	check(
		"LiteLLM snapshot SHA-256 matches pin",
		litellmBuffer !== undefined && sha256(litellmBuffer) === EXPECTED.litellm.sha256,
	);
	const modelsDevBuffer = tracked.get("rust/crates/ccusage-core/src/models-dev-pricing.json");
	check("models.dev snapshot present", modelsDevBuffer !== undefined);
	if (modelsDevBuffer) {
		const snapshot = JSON.parse(modelsDevBuffer.toString("utf8"));
		const entry = snapshot[EXPECTED.additiveEntry.name];
		check(
			"models.dev snapshot digest matches UPSTREAM.toml/manifest pin",
			modelsDevToml?.sha256 === sha256(modelsDevBuffer) && manifest.get("rust/crates/ccusage-core/src/models-dev-pricing.json") === sha256(modelsDevBuffer),
		);
		check(
			"models.dev snapshot diverges from pristine only additively (entry count)",
			Object.keys(snapshot).length === EXPECTED.modelsDev.entries,
		);
		check(
			"additive antigravity pricing entry exact (gemini-3.1-pro)",
			JSON.stringify(entry) === JSON.stringify({ cost: EXPECTED.additiveEntry.cost, limit: EXPECTED.additiveEntry.limit }),
			`got: ${JSON.stringify(entry)}`,
		);
	}

	// --- 5. offline pricing ---------------------------------------------------
	const buildRs = tracked.get("rust/crates/ccusage-core/build.rs")?.toString("utf8") ?? "";
	check(
		"vendored LiteLLM pricing fallback present in core build.rs",
		buildRs.includes("pricing/litellm-pricing.json"),
	);
	const coreCargoToml = tracked.get("rust/crates/ccusage-core/Cargo.toml")?.toString("utf8") ?? "";
	const featuresMatch = coreCargoToml.match(/\[features\]\s*(?:default\s*=\s*\[\s*\])?/);
	check(
		"fetch-litellm-pricing feature exists but is not default in vendored core",
		coreCargoToml.includes('fetch-litellm-pricing = ["dep:minreq"]') && featuresMatch !== null,
	);
	const productManifest = fs.readFileSync(srcTauriManifest, "utf8");
	check(
		"product does not enable fetch-litellm-pricing",
		!productManifest.includes("fetch-litellm-pricing"),
	);
	let minreqAbsent = false;
	try {
		cargoTree("-i", "minreq");
	} catch (error) {
		minreqAbsent = String(error.stderr).includes("did not match any packages");
	}
	check("minreq absent from product dependency graph (fetch feature off)", minreqAbsent);

	// --- 6. SQLite linkage ----------------------------------------------------
	let libsqlite3Absent = false;
	try {
		cargoTree("-i", "libsqlite3-sys");
	} catch (error) {
		libsqlite3Absent = String(error.stderr).includes("did not match any packages");
	}
	check("libsqlite3-sys absent from product dependency graph", libsqlite3Absent);
	let sqlite3SrcTree = "";
	let sqlite3SrcOk = true;
	try {
		sqlite3SrcTree = cargoTree("-i", "sqlite3-src");
	} catch {
		sqlite3SrcOk = false;
	}
	const sqlite3SrcCount = (sqlite3SrcTree.match(/^sqlite3-src v/gm) ?? []).length;
	check(
		"exactly one sqlite3-src native linkage in product dependency graph",
		sqlite3SrcOk && sqlite3SrcCount === 1,
		`roots: ${sqlite3SrcCount}`,
	);

	console.log("");
	if (failures.length) {
		console.error(`vendor:verify FAILED — ${failures.length} check(s): ${failures.join("; ")}`);
		process.exit(1);
	}
	console.log("vendor:verify PASS — vendored ccusage tree, patches, pricing and linkage all verified.");
}

main();
