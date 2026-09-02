type Ecosystem = "npm" | "rust";
type Severity = "critical" | "high" | "moderate" | "low" | "unknown";

export interface AuditFinding {
  ecosystem: Ecosystem;
  id: string;
  package: string;
  severity: Severity;
  title: string;
  source?: string;
}

export interface AuditException {
  ecosystem: Ecosystem;
  id: string;
  package: string;
  expires: string;
  reason: string;
  upgrade: string;
}

interface Allowlist {
  reviewed: string;
  exceptions: AuditException[];
}

interface BunAdvisory {
  severity?: string;
  title?: string;
  url?: string;
  id?: number | string;
}

interface CargoAuditReport {
  vulnerabilities?: {
    list?: Array<{
      advisory?: { id?: string; title?: string };
      package?: { name?: string };
    }>;
  };
  warnings?: Record<string, unknown[]>;
}

const BLOCKING_SEVERITIES = new Set<Severity>(["critical", "high"]);

function isIsoDate(value: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const date = new Date(`${value}T00:00:00Z`);
  return !Number.isNaN(date.valueOf()) && date.toISOString().slice(0, 10) === value;
}

export function parseAllowlist(value: unknown): Allowlist {
  if (!value || typeof value !== "object") throw new Error("dependency audit allowlist must be an object");
  const input = value as Record<string, unknown>;
  if (typeof input.reviewed !== "string" || !isIsoDate(input.reviewed)) {
    throw new Error("dependency audit allowlist reviewed must be a real YYYY-MM-DD date");
  }
  if (!Array.isArray(input.exceptions)) {
    throw new Error("dependency audit allowlist exceptions must be an array");
  }
  const exceptions = input.exceptions.map((value, index) => {
    if (!value || typeof value !== "object") throw new Error(`exception ${index} must be an object`);
    const item = value as Record<string, unknown>;
    if (item.ecosystem !== "npm" && item.ecosystem !== "rust") {
      throw new Error(`exception ${index} has invalid ecosystem`);
    }
    for (const field of ["id", "package", "reason", "upgrade"] as const) {
      if (typeof item[field] !== "string" || !item[field].trim()) {
        throw new Error(`exception ${index} must include ${field}`);
      }
    }
    if (typeof item.expires !== "string" || !isIsoDate(item.expires)) {
      throw new Error(`exception ${index} has invalid expiry`);
    }
    return item as unknown as AuditException;
  });
  return { reviewed: input.reviewed, exceptions };
}

function exceptionKey(item: Pick<AuditFinding | AuditException, "ecosystem" | "id" | "package">) {
  return `${item.ecosystem}:${item.id}:${item.package}`;
}

function normalizeSeverity(value: string | undefined): Severity {
  return value === "critical" || value === "high" || value === "moderate" || value === "low"
    ? value
    : "unknown";
}

export function parseBunAudit(report: Record<string, BunAdvisory[]>): AuditFinding[] {
  return Object.entries(report).flatMap(([packageName, advisories]) =>
    advisories.map((advisory) => ({
      ecosystem: "npm" as const,
      id: advisory.url?.split("/").at(-1) ?? String(advisory.id ?? "unknown"),
      package: packageName,
      severity: normalizeSeverity(advisory.severity),
      title: advisory.title ?? "Untitled npm advisory",
    })),
  );
}

export function parseCargoAudit(report: CargoAuditReport, source: string): AuditFinding[] {
  return (report.vulnerabilities?.list ?? []).map((item) => ({
    ecosystem: "rust" as const,
    id: item.advisory?.id ?? "unknown",
    package: item.package?.name ?? "unknown",
    // RustSec does not require a CVSS score. cargo-audit's vulnerability set is fail-closed.
    severity: "unknown" as const,
    title: item.advisory?.title ?? "Untitled RustSec advisory",
    source,
  }));
}

export function evaluateFindings(
  findings: AuditFinding[],
  exceptions: AuditException[],
  today: string,
) {
  const exceptionMap = new Map<string, AuditException>();
  const invalidExceptions: string[] = [];

  for (const exception of exceptions) {
    const key = exceptionKey(exception);
    if (exceptionMap.has(key)) invalidExceptions.push(`${key} is duplicated`);
    if (!isIsoDate(exception.expires)) {
      invalidExceptions.push(`${key} has invalid expiry ${exception.expires}`);
    }
    if (!exception.reason.trim() || !exception.upgrade.trim()) {
      invalidExceptions.push(`${key} must include reason and upgrade guidance`);
    }
    exceptionMap.set(key, exception);
  }

  const accepted: Array<{ finding: AuditFinding; exception: AuditException }> = [];
  const blocked: AuditFinding[] = [];
  const reported: AuditFinding[] = [];
  const seen = new Set<string>();

  for (const finding of findings) {
    const key = exceptionKey(finding);
    const exception = exceptionMap.get(key);
    const blocks = finding.ecosystem === "rust" || BLOCKING_SEVERITIES.has(finding.severity);
    if (!blocks) {
      reported.push(finding);
      continue;
    }
    if (exception && exception.expires >= today) {
      accepted.push({ finding, exception });
      seen.add(key);
    } else {
      blocked.push(finding);
    }
  }

  const stale = exceptions.filter((exception) => !seen.has(exceptionKey(exception)));
  return { accepted, blocked, reported, stale, invalidExceptions };
}

function runJson(command: string[], acceptedExitCodes: number[]): unknown {
  const result = Bun.spawnSync(command, { stdout: "pipe", stderr: "pipe" });
  if (!acceptedExitCodes.includes(result.exitCode)) {
    const stderr = result.stderr.toString().trim();
    throw new Error(`${command.join(" ")} failed (${result.exitCode}): ${stderr}`);
  }
  const stdout = result.stdout.toString().trim();
  try {
    return JSON.parse(stdout);
  } catch (error) {
    throw new Error(`${command.join(" ")} did not return valid JSON: ${String(error)}`);
  }
}

function printFinding(prefix: string, finding: AuditFinding) {
  const source = finding.source ? ` (${finding.source})` : "";
  console.log(`${prefix} ${finding.ecosystem}/${finding.severity} ${finding.id} ${finding.package}${source}`);
  console.log(`  ${finding.title}`);
}

async function main() {
  const allowlistPath = new URL("./dependency-audit-allowlist.json", import.meta.url);
  const allowlist = parseAllowlist(await Bun.file(allowlistPath).json());
  const findings = parseBunAudit(
    runJson(["bun", "audit", "--json"], [0, 1]) as Record<string, BunAdvisory[]>,
  );

  const cargoLocks = [
    "packages/accelerator/core/Cargo.lock",
    "packages/accelerator/server/Cargo.lock",
    "packages/accelerator/src-tauri/Cargo.lock",
  ];
  let rustWarningCount = 0;
  for (const lock of cargoLocks) {
    const report = runJson(["cargo", "audit", "--json", "--file", lock], [0, 1]) as CargoAuditReport;
    findings.push(...parseCargoAudit(report, lock));
    rustWarningCount += Object.values(report.warnings ?? {}).reduce(
      (count, warnings) => count + warnings.length,
      0,
    );
  }

  const today = new Date().toISOString().slice(0, 10);
  const result = evaluateFindings(findings, allowlist.exceptions, today);

  for (const { finding, exception } of result.accepted) {
    printFinding(`ACCEPTED until ${exception.expires}:`, finding);
  }
  for (const finding of result.reported) printFinding("REPORT:", finding);
  for (const finding of result.blocked) printFinding("BLOCKED:", finding);
  for (const exception of result.stale) {
    console.log(`STALE EXCEPTION: ${exceptionKey(exception)} (remove it if the finding is gone)`);
  }
  for (const error of result.invalidExceptions) console.error(`INVALID EXCEPTION: ${error}`);

  console.log(
    `Dependency audit: ${result.blocked.length} blocked, ${result.accepted.length} accepted, ${result.reported.length} moderate/low reported, ${rustWarningCount} RustSec informational warnings.`,
  );

  if (result.blocked.length || result.invalidExceptions.length) process.exit(1);
}

if (import.meta.main) {
  await main();
}
