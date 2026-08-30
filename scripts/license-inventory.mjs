// Generate docs/V0.3_LICENSE_INVENTORY.md: the license inventory for the full
// Rust dependency graph of the product (registry crates + vendored path
// crates), per docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md section 10.
//
// Offline: reads `cargo metadata --offline --filter-platform
// x86_64-pc-windows-msvc`. Vendored ccusage crates carry no `license` manifest
// field (they are `publish = false` path crates); their license is the
// vendored MIT LICENSE file, which this script verifies exists.
//
// Blocking policy (Gate 0):
//   - unknown / unlicensed — block (nothing may fall through without a license);
//   - GPL / AGPL / LGPL — block (no approvals configured);
//   - MPL-2.0 — allowed, file-level copyleft notice obligation recorded;
//   - permissive (MIT, Apache-2.0, BSD, ISC, Zlib, CC0, Unicode-3.0,
//     Unlicense, MIT-0, 0BSD) — allowed; Apache-2.0 NOTICE obligations recorded.
//
// Any blocking finding exits non-zero. This stage does not produce the final
// installer THIRD-PARTY-NOTICES bundle (Phase 4+/release duty, recorded in the
// output document).

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const srcTauriManifest = path.join(repoRoot, "src-tauri", "Cargo.toml");
const outPath = path.join(repoRoot, "docs", "V0.3_LICENSE_INVENTORY.md");

const HOST_TARGET = "x86_64-pc-windows-msvc";

const ALLOWED = new Set([
	"MIT", "MIT-0", "Apache-2.0", "ISC", "Zlib", "CC0-1.0", "0BSD", "Unlicense",
	"Unicode-3.0", "Unicode-DFS-2016", "BSD-1-Clause", "BSD-2-Clause", "BSD-3-Clause",
]);
// Allowed, but with distribution notice obligations that must be honoured by
// the final installer's THIRD_PARTY_NOTICES bundle.
const NOTICE_REQUIRED = new Set(["Apache-2.0", "MPL-2.0", "BSD-3-Clause", "BSD-2-Clause", "ISC", "Unicode-3.0", "Zlib"]);
const BLOCKED_PREFIXES = ["GPL", "AGPL", "LGPL"];

const DIRECT_SOURCES = [
	{
		name: "ccusage v20.0.20 (vendored Rust sources)",
		license: "MIT",
		copyright: "Copyright (c) 2025 ryoppippi",
		evidence: "src-tauri/vendor/ccusage/LICENSE (upstream apps/ccusage/LICENSE at bd7f89b469aee5635fb2e6722dd6d70f2d113ac1)",
	},
	{
		name: "Antigravity adapter port (PR #1487, unmerged downstream patch)",
		license: "MIT",
		copyright: "Copyright (c) 2025 ryoppippi (fork sambitcreate/ccusage at c58c1b3aab2eacc82add250c8229bb6192e4489b retains the same MIT license)",
		evidence: "src-tauri/vendor/ccusage/patches/0001-antigravity-c58c1b3.patch provenance in UPSTREAM.toml",
	},
	{
		name: "LiteLLM pricing snapshot (model_prices_and_context_window.json)",
		license: "MIT (per repository licensing outside the enterprise directory)",
		copyright: "Copyright (c) Berri AI",
		evidence: "src-tauri/vendor/ccusage/pricing/pricing-manifest.json (commit 1a183efaa1a2108aed7e1bed8d445d93bd1aa60d)",
	},
	{
		name: "models.dev pricing snapshot (models-dev-pricing.json)",
		license: "MIT",
		copyright: "Copyright (c) 2025 models.dev",
		evidence: "docs/V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md section 10; vendored snapshot digest in pricing-manifest.json",
	},
];

function splitTopLevel(expression, separator) {
	const parts = [];
	let depth = 0;
	let current = "";
	let index = 0;
	while (index < expression.length) {
		const ch = expression[index];
		if (ch === "(") depth++;
		if (ch === ")") depth--;
		if (depth === 0 && expression.startsWith(separator, index)) {
			parts.push(current.trim());
			current = "";
			index += separator.length;
			continue;
		}
		current += ch;
		index++;
	}
	parts.push(current.trim());
	return parts;
}

function families(expression) {
	// All license tokens appearing anywhere in the SPDX expression.
	const tokens = new Set();
	for (const orBranch of splitTopLevel(expression, " OR ")) {
		for (const andBranch of splitTopLevel(orBranch, " AND ")) {
			for (const alt of splitTopLevel(andBranch, "/")) {
				const token = alt.replace(/[()]/g, "").trim();
				if (token && token !== "WITH") tokens.add(token);
			}
		}
	}
	return tokens;
}

// Parse an SPDX expression into OR-branches of AND-conjuncts, each conjunct
// holding its `/`-alts (legacy `MIT/Apache-2.0` means the same thing as
// `MIT OR Apache-2.0`). Shape: branch[] = conjunct[] = token[].
function parseExpression(expression) {
	const orParts = splitTopLevel(expression, " OR ");
	if (orParts.length > 1) return orParts.flatMap(parseExpression);
	const andParts = splitTopLevel(expression, " AND ");
	if (andParts.length > 1) {
		let branches = [[]];
		for (const part of andParts) {
			branches = branches.flatMap((prefix) => parseExpression(part).map((branch) => [...prefix, ...branch]));
		}
		return branches;
	}
	const stripped = expression.replace(/^\(+/, "").replace(/\)+$/, "").trim();
	if (stripped !== expression.trim()) return parseExpression(stripped);
	const alts = splitTopLevel(stripped, "/")
		.map((token) => token.trim())
		.filter(Boolean);
	return [[alts]];
}

function classify(expression) {
	const branches = parseExpression(expression);
	const acceptable = (alts) => alts.some((alt) => ALLOWED.has(alt) || NOTICE_REQUIRED.has(alt));
	const branchVerdicts = branches.map((conjuncts) => {
		const verdicts = conjuncts.map((alts) =>
			alts.every((alt) => ALLOWED.has(alt) || NOTICE_REQUIRED.has(alt))
				? "allowed"
				: acceptable(alts)
					? "mixed"
					: "blocked",
		);
		if (verdicts.some((v) => v === "blocked")) return "blocked";
		return verdicts.every((v) => v === "allowed") ? "allowed" : "mixed";
	});
	if (branchVerdicts.includes("allowed")) return "allowed";
	if (branchVerdicts.includes("mixed")) return "mixed";
	return "blocked";
}

function main() {
	const meta = JSON.parse(
		execFileSync(
			"cargo",
			["metadata", "--offline", "--format-version", "1", "--manifest-path", srcTauriManifest, "--filter-platform", HOST_TARGET],
			{ cwd: repoRoot, maxBuffer: 256 * 1024 * 1024 },
		).toString("utf8"),
	);

	const graphIds = new Set(meta.resolve.nodes.map((node) => node.id));
	const inGraph = meta.packages.filter((pkg) => graphIds.has(pkg.id));

	const vendoredLicense = fs.readFileSync(path.join(repoRoot, "src-tauri", "vendor", "ccusage", "LICENSE"), "utf8");
	if (!vendoredLicense.includes("MIT") || !vendoredLicense.includes("ryoppippi")) {
		throw new Error("vendored ccusage LICENSE no longer reads as MIT / ryoppippi; investigate before regenerating");
	}

	const rows = [];
	const blocked = [];
	const obligations = new Map();
	for (const pkg of inGraph) {
		const isHost = !pkg.source && pkg.name === "coding-agent-monitor";
		const isVendoredCcusage = !pkg.source && pkg.name.startsWith("ccusage-");
		if (isHost) continue;
		if (isVendoredCcusage) {
			rows.push({ name: pkg.name, version: pkg.version, license: "MIT (vendored, see vendor/ccusage/LICENSE)", source: "path", verdict: "allowed" });
			continue;
		}
		const expression = pkg.license ?? (pkg.license_file ? `file:${pkg.license_file}` : null);
		if (!expression) {
			blocked.push({ name: pkg.name, version: pkg.version, reason: "no license expression in manifest" });
			rows.push({ name: pkg.name, version: pkg.version, license: "(none)", source: "registry", verdict: "blocked" });
			continue;
		}
		const fams = families(expression);
		const verdict = classify(expression);
		const gpl = [...fams].find((token) => BLOCKED_PREFIXES.some((prefix) => token.startsWith(prefix)));
		if (verdict !== "allowed" || gpl) {
			blocked.push({
				name: pkg.name,
				version: pkg.version,
				reason: gpl ? `copyleft family ${gpl}` : `no allowed branch (verdict: ${verdict})`,
			});
		}
		for (const token of fams) {
			if (NOTICE_REQUIRED.has(token)) {
				if (!obligations.has(token)) obligations.set(token, []);
				obligations.get(token).push(pkg.name);
			}
		}
		rows.push({
			name: pkg.name,
			version: pkg.version,
			license: expression,
			source: pkg.source ? "registry" : "path",
			verdict: verdict !== "allowed" || gpl ? "blocked" : "allowed",
		});
	}

	rows.sort((a, b) => a.name.localeCompare(b.name));
	const counts = {};
	for (const row of rows) counts[row.verdict] = (counts[row.verdict] ?? 0) + 1;

	const generatedAt = new Date().toISOString().slice(0, 10);
	const lines = [];
	lines.push("# v0.3 Rust 依赖许可证 inventory");
	lines.push("");
	lines.push(`由 \`node scripts/license-inventory.mjs\` 生成（${generatedAt}），数据源：`);
	lines.push("`cargo metadata --offline --filter-platform x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`。");
	lines.push("覆盖 registry crate 与 vendored path crate（host crate 自身除外）。");
	lines.push("");
	lines.push(`总计 ${rows.length} 个第三方/ vendored 包：${counts.allowed ?? 0} allowed，${counts.blocked ?? 0} blocked。`);
	lines.push("");
	lines.push("## 直接来源映射（非 registry crate）");
	lines.push("");
	lines.push("| 来源 | 许可证 | 版权 | 证据位置 |");
	lines.push("| --- | --- | --- | --- |");
	for (const item of DIRECT_SOURCES) {
		lines.push(`| ${item.name} | ${item.license} | ${item.copyright} | ${item.evidence} |`);
	}
	lines.push("");
	lines.push("## 汇总（按 SPDX 家族出现次数）");
	lines.push("");
	const familyCounts = {};
	for (const row of rows) {
		for (const token of families(row.license.startsWith("file:") ? "" : row.license)) {
			familyCounts[token] = (familyCounts[token] ?? 0) + 1;
		}
	}
	for (const [token, count] of Object.entries(familyCounts).sort((a, b) => b[1] - a[1])) {
		lines.push(`- ${token}: ${count}`);
	}
	lines.push("");
	lines.push("## 逐包清单");
	lines.push("");
	lines.push("| 包 | 版本 | 来源 | 许可证表达式 | 判定 |");
	lines.push("| --- | --- | --- | --- | --- |");
	for (const row of rows) {
		lines.push(`| ${row.name} | ${row.version} | ${row.source} | ${row.license} | ${row.verdict} |`);
	}
	lines.push("");
	lines.push("## 后续 notice 义务（最终安装包，Phase 4+/发布前）");
	lines.push("");
	lines.push("本阶段不生成最终安装包的 `THIRD_PARTY_NOTICES.md`；发布前必须：");
	lines.push("");
	lines.push("1. 为下列家族的包完整保留版权与许可文本（Apache-2.0 有 NOTICE 文件的还需附带 NOTICE）：");
	for (const [token, names] of [...obligations.entries()].sort()) {
		lines.push(`   - ${token}（${names.length} 包，例如 ${names.slice(0, 5).join(", ")}${names.length > 5 ? " …" : ""}）`);
	}
	lines.push("2. MPL-2.0 为文件级 copyleft：保留对应源文件的许可头即可，无需开源宿主工程；发布前人工复核清单。");
	lines.push("3. 将第 10 节四类直接来源（ccusage、Antigravity 移植、LiteLLM、models.dev）的版权/许可文本并入 notices。");
	lines.push("4. 重新运行本脚本确认 blocked 计数为 0。");
	lines.push("");

	if (blocked.length) {
		lines.push("## 阻断项（Gate 0 不通过）");
		lines.push("");
		for (const item of blocked) {
			lines.push(`- **${item.name} ${item.version}** — ${item.reason}`);
		}
		lines.push("");
	}

	fs.writeFileSync(outPath, `${lines.join("\n")}\n`);
	console.log(`license inventory written to ${path.relative(repoRoot, outPath)}`);
	console.log(`packages: ${rows.length} (${counts.allowed ?? 0} allowed, ${counts.blocked ?? 0} blocked)`);
	if (blocked.length) {
		console.error(`BLOCKED — ${blocked.length} package(s):`);
		for (const item of blocked) console.error(`  ${item.name} ${item.version}: ${item.reason}`);
		process.exit(1);
	}
}

main();
