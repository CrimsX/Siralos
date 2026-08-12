import { describe, expect, it } from "vitest";
import {
  fileUriToPath,
  mirrorUriToWorkspaceRelative,
  pathToFileUri,
  workspaceRelativeToMirrorUri,
} from "./file-uri.js";

const MIRROR = process.platform === "win32" ? "C:\\siralos\\mirror-1" : "/tmp/siralos/mirror-1";

describe("file-uri conversion", () => {
  it("converts POSIX paths to file URIs and back", () => {
    const uri = pathToFileUri("/tmp/siralos/mirror-1/src/player.gd");
    expect(uri).toBe("file:///tmp/siralos/mirror-1/src/player.gd");
    expect(fileUriToPath(uri)).toBe("/tmp/siralos/mirror-1/src/player.gd");
  });

  it("handles Windows drive paths", () => {
    if (process.platform !== "win32") {
      return;
    }
    const uri = pathToFileUri("C:\\siralos\\mirror-1\\player.gd");
    expect(fileUriToPath(uri)).toBe("C:\\siralos\\mirror-1\\player.gd");
  });

  it("percent-encodes spaces and Unicode without double-encoding", () => {
    const uri = pathToFileUri("/tmp/siralos/mirror/my file.gd");
    expect(uri).toBe("file:///tmp/siralos/mirror/my%20file.gd");
    expect(fileUriToPath(uri)).toBe("/tmp/siralos/mirror/my file.gd");
    const unicode = pathToFileUri("/tmp/siralos/mirror/плаyer.gd");
    expect(fileUriToPath(unicode)).toBe("/tmp/siralos/mirror/плаyer.gd");
  });

  it("rejects non-file schemes, host authorities, and malformed percent encoding", () => {
    expect(fileUriToPath("http://127.0.0.1/x")).toBeNull();
    expect(fileUriToPath("file://evil-host/x.gd")).toBeNull();
    expect(fileUriToPath("file:///x%zz.gd")).toBeNull();
    expect(fileUriToPath("file://")).toBeNull();
  });

  it("maps mirror URIs to workspace-relative paths and rejects out-of-mirror URIs", () => {
    const uri = pathToFileUri(`${MIRROR}/src/player/player.gd`);
    expect(mirrorUriToWorkspaceRelative(uri, MIRROR)).toBe("src/player/player.gd");
    expect(mirrorUriToWorkspaceRelative("file:///elsewhere/x.gd", MIRROR)).toBeNull();
    expect(mirrorUriToWorkspaceRelative(pathToFileUri(MIRROR), MIRROR)).toBeNull();
    expect(mirrorUriToWorkspaceRelative("http://x/y.gd", MIRROR)).toBeNull();
  });

  it("maps workspace-relative paths to mirror URIs without traversal", () => {
    const uri = workspaceRelativeToMirrorUri("src/player/player.gd", MIRROR);
    expect(uri).not.toBeNull();
    if (uri !== null) {
      expect(mirrorUriToWorkspaceRelative(uri, MIRROR)).toBe("src/player/player.gd");
    }
    expect(workspaceRelativeToMirrorUri("../escape.gd", MIRROR)).toBeNull();
    expect(workspaceRelativeToMirrorUri("/abs.gd", MIRROR)).toBeNull();
    expect(workspaceRelativeToMirrorUri("C:\\abs.gd", MIRROR)).toBeNull();
    expect(workspaceRelativeToMirrorUri("", MIRROR)).toBeNull();
  });
});

describe("file-uri traversal rejection", () => {
  it("rejects decoded .. segments that would escape the mirror root", () => {
    expect(
      mirrorUriToWorkspaceRelative(pathToFileUri(`${MIRROR}/../secret.gd`), MIRROR),
    ).toBeNull();
    expect(
      mirrorUriToWorkspaceRelative(pathToFileUri(`${MIRROR}/src/../secret.gd`), MIRROR),
    ).toBeNull();
  });
});
