import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const outputPath = resolve(
  repoRoot,
  process.argv[2] ?? "token-usage-last7days.json",
);
const endDate = process.argv[3] ?? "2026-08-25";
const end = new Date(`${endDate}T00:00:00Z`);
if (Number.isNaN(end.getTime())) throw new Error(`Invalid end date: ${endDate}`);
const start = new Date(end);
start.setUTCDate(start.getUTCDate() - 6);
const startDate = start.toISOString().slice(0, 10);

const unifiedBinary = resolve(
  repoRoot,
  "src-tauri/target/release/ccusage.exe",
);
const antigravityBinary = resolve(
  repoRoot,
  "src-tauri/target/release/ccusage-antigravity.exe",
);
const runJson = (binary, args) =>
  JSON.parse(execFileSync(binary, args, { encoding: "utf8" }));

const unified = runJson(unifiedBinary, [
  "daily",
  "--json",
  "--offline",
  "--by-agent",
  "--since",
  startDate,
  "--until",
  endDate,
]);
const antigravity = runJson(antigravityBinary, [
  "antigravity",
  "daily",
  "--json",
  "--offline",
  "--since",
  startDate,
  "--until",
  endDate,
]);

const zero = () => ({
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  totalTokens: 0,
  unattributedTokens: 0,
  totalCost: 0,
});
const tokenFields = [
  "inputTokens",
  "outputTokens",
  "cacheReadTokens",
  "cacheCreationTokens",
  "totalTokens",
];
const add = (target, source) => {
  for (const key of Object.keys(zero())) target[key] += Number(source[key] ?? 0);
};
const modelKey = (agent, modelName) => `${agent}\u0000${modelName}`;
const days = new Map();
for (let cursor = new Date(start); cursor <= end; cursor.setUTCDate(cursor.getUTCDate() + 1)) {
  const date = cursor.toISOString().slice(0, 10);
  days.set(date, { date, ...zero(), agents: [] });
}

const addAgent = (date, agentId, row) => {
  const day = days.get(date);
  if (!day) return;
  const agent = {
    agent: agentId,
    inputTokens: Number(row.inputTokens ?? 0),
    outputTokens: Number(row.outputTokens ?? 0),
    cacheReadTokens: Number(row.cacheReadTokens ?? 0),
    cacheCreationTokens: Number(row.cacheCreationTokens ?? 0),
    totalTokens: Number(row.totalTokens ?? 0),
    totalCost: Number(row.totalCost ?? 0),
    modelBreakdowns: (row.modelBreakdowns ?? []).map((model) => ({
      modelName: model.modelName,
      inputTokens: Number(model.inputTokens ?? 0),
      outputTokens: Number(model.outputTokens ?? 0),
      cacheReadTokens: Number(model.cacheReadTokens ?? 0),
      cacheCreationTokens: Number(model.cacheCreationTokens ?? 0),
      totalTokens:
        Number(model.inputTokens ?? 0) +
        Number(model.outputTokens ?? 0) +
        Number(model.cacheReadTokens ?? 0) +
        Number(model.cacheCreationTokens ?? 0),
      cost: Number(model.cost ?? 0),
    })),
  };
  agent.unattributedTokens =
    agent.totalTokens -
    agent.modelBreakdowns.reduce((sum, model) => sum + model.totalTokens, 0);
  day.agents.push(agent);
  add(day, agent);
};

for (const row of unified.daily ?? []) {
  for (const agent of row.agents ?? []) addAgent(row.period, agent.agent, agent);
}
for (const row of antigravity.daily ?? []) addAgent(row.date, "antigravity", row);

for (const day of days.values()) {
  const agentMap = new Map();
  for (const agent of day.agents) {
    const existing = agentMap.get(agent.agent);
    if (!existing) {
      agentMap.set(agent.agent, agent);
      continue;
    }
    add(existing, agent);
    const modelMap = new Map(existing.modelBreakdowns.map((m) => [m.modelName, m]));
    for (const model of agent.modelBreakdowns) {
      const prior = modelMap.get(model.modelName);
      if (prior) {
        for (const key of ["inputTokens", "outputTokens", "cacheReadTokens", "cacheCreationTokens", "totalTokens", "cost"]) prior[key] += model[key];
      } else {
        modelMap.set(model.modelName, model);
      }
    }
    existing.modelBreakdowns = [...modelMap.values()];
  }
  day.agents = [...agentMap.values()].sort((a, b) => a.agent.localeCompare(b.agent));
  day.unattributedTokens = day.agents.reduce(
    (sum, agent) => sum + agent.unattributedTokens,
    0,
  );
  for (const agent of day.agents) {
    agent.modelBreakdowns.sort((a, b) => b.totalTokens - a.totalTokens || a.modelName.localeCompare(b.modelName));
  }
}

const totals = zero();
const agentTotals = new Map();
const modelTotals = new Map();
for (const day of days.values()) {
  for (const agent of day.agents) {
    add(totals, agent);
    const agentTotal = agentTotals.get(agent.agent) ?? { agent: agent.agent, ...zero(), modelBreakdowns: [] };
    add(agentTotal, agent);
    agentTotals.set(agent.agent, agentTotal);
    for (const model of agent.modelBreakdowns) {
      const key = modelKey(agent.agent, model.modelName);
      const modelTotal = modelTotals.get(key) ?? {
        agent: agent.agent,
        modelName: model.modelName,
        inputTokens: 0,
        outputTokens: 0,
        cacheReadTokens: 0,
        cacheCreationTokens: 0,
        totalTokens: 0,
        cost: 0,
      };
      for (const key of tokenFields) modelTotal[key] += model[key];
      modelTotal.cost += model.cost;
      modelTotals.set(key, modelTotal);
    }
  }
}
for (const day of days.values()) {
  day.totalCost = day.totalCost;
  delete day.modelBreakdowns;
}

const result = {
  schemaVersion: 1,
  generatedAt: new Date().toISOString(),
  period: { startDate, endDate, days: 7 },
  includedAgents: [...agentTotals.keys()].sort(),
  sources: {
    unified: "ccusage daily --json --offline --by-agent",
    antigravity: "ccusage antigravity daily --json --offline",
  },
  daily: [...days.values()],
  totals: {
    ...totals,
    agents: [...agentTotals.values()]
      .sort((a, b) => b.totalTokens - a.totalTokens)
      .map((agent) => ({ ...agent, modelBreakdowns: undefined }))
      .map(({ modelBreakdowns, ...agent }) => agent),
    modelBreakdowns: [...modelTotals.values()].sort(
      (a, b) => b.totalTokens - a.totalTokens || a.agent.localeCompare(b.agent) || a.modelName.localeCompare(b.modelName),
    ),
  },
};

mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(result, null, 2)}\n`, "utf8");
console.log(`Wrote ${outputPath}`);
