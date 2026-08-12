import { describe, expect, it } from "vitest";
import { CheckpointStorageLimitError } from "@siralos/adapters";

describe("@siralos/adapters public checkpoint exports", () => {
  it("exposes the typed storage-limit refusal through the package entry point", () => {
    const error = new CheckpointStorageLimitError("storage limit reached");
    expect(error).toBeInstanceOf(Error);
    expect(error).toBeInstanceOf(CheckpointStorageLimitError);
    expect(error.name).toBe("CheckpointStorageLimitError");
    expect(error.message).toBe("storage limit reached");
  });
});
