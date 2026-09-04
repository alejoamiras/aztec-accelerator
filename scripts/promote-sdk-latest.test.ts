import { describe, expect, test } from "bun:test";
import {
  assertFreshPromotionState,
  isActiveWorkflowStatus,
  resolveRemoteTagCommit,
} from "./promote-sdk-latest.ts";

describe("SDK latest promotion", () => {
  test("prefers the peeled commit for an annotated tag", () => {
    expect(
      resolveRemoteTagCommit(
        "tag-object\trefs/tags/@scope/pkg@1.0.0\ncommit\trefs/tags/@scope/pkg@1.0.0^{}\n",
      ),
    ).toBe("commit");
  });

  test("uses a lightweight tag commit", () => {
    expect(resolveRemoteTagCommit("commit\trefs/tags/@scope/pkg@1.0.0\n")).toBe("commit");
  });

  test("recognizes every non-terminal GitHub workflow status", () => {
    for (const status of ["queued", "in_progress", "pending", "requested", "waiting"]) {
      expect(isActiveWorkflowStatus(status)).toBe(true);
    }
    expect(isActiveWorkflowStatus("completed")).toBe(false);
  });

  test("refuses stale latest or a candidate that moved off testnet", () => {
    expect(() =>
      assertFreshPromotionState(
        { latest: "5.1.0", testnet: "5.2.0" },
        { latest: "5.1.1", testnet: "5.2.0" },
        "5.2.0",
        false,
      ),
    ).toThrow("latest changed");
    expect(() =>
      assertFreshPromotionState(
        { latest: "5.1.0", testnet: "5.2.0" },
        { latest: "5.1.0", testnet: "5.3.0" },
        "5.2.0",
        false,
      ),
    ).toThrow("testnet changed");
    expect(() =>
      assertFreshPromotionState(
        { latest: "5.2.0", testnet: "5.3.0" },
        { latest: "5.2.0", testnet: "5.4.0" },
        "5.1.0",
        true,
      ),
    ).not.toThrow();
  });
});
