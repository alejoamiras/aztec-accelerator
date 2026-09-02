export const SDK_PACKAGE = "@alejoamiras/aztec-accelerator";
export const SDK_RELEASE_WORKFLOW = ".github/workflows/release-sdk.yml";
export const LEGACY_SDK_RELEASE_WORKFLOW = ".github/workflows/publish-testnet.yml";
export const SDK_REPOSITORY = "https://github.com/alejoamiras/aztec-accelerator";
export const SDK_SOURCE_DEPENDENCY =
  "git+https://github.com/alejoamiras/aztec-accelerator@refs/heads/main";
export const SDK_VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-revision\.\d+)?$/;

interface AttestationResponse {
  attestations?: Array<{
    predicateType?: string;
    bundle?: {
      dsseEnvelope?: { payload?: string };
    };
  }>;
}

interface ProvenanceStatement {
  subject?: Array<{ name?: string; digest?: { sha512?: string } }>;
  predicate?: {
    buildDefinition?: {
      externalParameters?: {
        workflow?: { ref?: string; repository?: string; path?: string };
      };
      resolvedDependencies?: Array<{ uri?: string; digest?: { gitCommit?: string } }>;
    };
  };
}

export interface VerifiedProvenance {
  commit: string;
  ref: string;
  repository: string;
  workflow: string;
}

function npmView(spec: string, field: string): string {
  const result = Bun.spawnSync(["npm", "view", spec, field], {
    stdout: "pipe",
    stderr: "pipe",
  });
  if (result.exitCode !== 0) {
    throw new Error(`npm view failed: ${result.stderr.toString().trim()}`);
  }
  const value = result.stdout.toString().trim();
  if (!value) throw new Error(`npm view ${spec} ${field} returned an empty value`);
  return value;
}

export function verifyProvenanceStatement(
  statement: ProvenanceStatement,
  version: string,
  expectedCommit?: string,
  allowedWorkflows: readonly string[] = [SDK_RELEASE_WORKFLOW],
  expectedSha512?: string,
): VerifiedProvenance {
  const expectedSubject = `pkg:npm/%40alejoamiras/aztec-accelerator@${version}`;
  const subject = statement.subject?.find((subject) => subject.name === expectedSubject);
  if (!subject) {
    throw new Error(`provenance subject does not contain ${expectedSubject}`);
  }
  if (expectedSha512 && subject.digest?.sha512 !== expectedSha512) {
    throw new Error("provenance subject digest does not match the npm tarball integrity");
  }

  const definition = statement.predicate?.buildDefinition;
  const workflow = definition?.externalParameters?.workflow;
  if (workflow?.repository !== SDK_REPOSITORY) {
    throw new Error(`unexpected provenance repository: ${workflow?.repository ?? "missing"}`);
  }
  if (!workflow?.path || !allowedWorkflows.includes(workflow.path)) {
    throw new Error(`unexpected provenance workflow: ${workflow.path ?? "missing"}`);
  }
  if (workflow.ref !== "refs/heads/main") {
    throw new Error(`unexpected provenance ref: ${workflow.ref ?? "missing"}`);
  }

  const commit = definition?.resolvedDependencies?.find(
    (dependency) =>
      dependency.uri === SDK_SOURCE_DEPENDENCY && dependency.digest?.gitCommit,
  )?.digest?.gitCommit;
  if (!commit) throw new Error("provenance has no resolved git commit");
  if (!/^[0-9a-f]{40}$/.test(commit)) throw new Error(`provenance has invalid git commit ${commit}`);
  if (expectedCommit && commit !== expectedCommit) {
    throw new Error(`provenance commit ${commit} does not match expected ${expectedCommit}`);
  }

  return {
    commit,
    ref: workflow.ref,
    repository: workflow.repository,
    workflow: workflow.path,
  };
}

export async function fetchAndVerifySdkProvenance(
  version: string,
  expectedCommit?: string,
  allowedWorkflows?: readonly string[],
): Promise<VerifiedProvenance> {
  const url = npmView(`${SDK_PACKAGE}@${version}`, "dist.attestations.url");
  const integrity = npmView(`${SDK_PACKAGE}@${version}`, "dist.integrity");
  const match = /^sha512-([A-Za-z0-9+/]+={0,2})$/.exec(integrity);
  const encodedDigest = match?.[1];
  if (!encodedDigest) throw new Error(`unexpected npm integrity format: ${integrity}`);
  const expectedSha512 = Buffer.from(encodedDigest, "base64").toString("hex");
  if (expectedSha512.length !== 128) throw new Error("npm integrity is not a SHA-512 digest");
  const response = await fetch(url);
  if (!response.ok) throw new Error(`attestation request failed: HTTP ${response.status}`);
  const body = (await response.json()) as AttestationResponse;
  const attestation = body.attestations?.find(
    (item) => item.predicateType === "https://slsa.dev/provenance/v1",
  );
  const payload = attestation?.bundle?.dsseEnvelope?.payload;
  if (!payload) throw new Error("npm package has no SLSA provenance attestation payload");
  const statement = JSON.parse(Buffer.from(payload, "base64").toString("utf8"));
  return verifyProvenanceStatement(
    statement,
    version,
    expectedCommit,
    allowedWorkflows,
    expectedSha512,
  );
}

if (import.meta.main) {
  const version = process.argv[2];
  const expectedCommit = process.argv[3];
  if (!version) {
    console.error("usage: bun scripts/sdk-release-verification.ts <version> [expected-commit]");
    process.exit(1);
  }
  const verified = await fetchAndVerifySdkProvenance(version, expectedCommit);
  console.log(
    `verified npm provenance: ${verified.repository}/${verified.workflow}@${verified.ref} (${verified.commit})`,
  );
}
