import { describe, expect, it } from "vitest";
import {
  isWithinPathIdentity,
  normalizePathIdentity,
  samePathIdentity,
} from "./fs-path-identity.js";

describe("normalizePathIdentity", () => {
  it("strips the Windows extended-length prefix", () => {
    expect(normalizePathIdentity("\\\\?\\C:\\a\\b", "win32")).toBe("c:\\a\\b");
  });

  it("folds separators and drive-letter casing on Windows", () => {
    expect(normalizePathIdentity("c:/a/b", "win32")).toBe("c:\\a\\b");
    expect(normalizePathIdentity("C:\\A\\B", "win32")).toBe("c:\\a\\b");
  });

  it("folds case on case-insensitive platforms only", () => {
    expect(normalizePathIdentity("/A/B/C", "darwin")).toBe("/a/b/c");
    expect(normalizePathIdentity("/A/B/C", "linux")).toBe("/A/B/C");
  });

  it("keeps the drive root spelling", () => {
    expect(normalizePathIdentity("c:\\", "win32")).toBe("c:\\");
    expect(normalizePathIdentity("C:", "win32")).toBe("c:\\");
  });

  it("unifies a bare drive letter with a drive root", () => {
    expect(normalizePathIdentity("c:", "win32")).toBe("c:\\");
    expect(samePathIdentity("C:", "c:\\", "win32")).toBe(true);
    expect(samePathIdentity("C:\\", "c:", "win32")).toBe(true);
    expect(samePathIdentity("\\\\?\\C:\\", "c:", "win32")).toBe(true);
  });

  it("unifies extended-length UNC with plain UNC", () => {
    expect(normalizePathIdentity("\\\\?\\UNC\\server\\share\\x", "win32")).toBe(
      "\\\\server\\share\\x",
    );
    expect(samePathIdentity("\\\\?\\UNC\\Server\\Share\\x", "\\\\server\\share\\x", "win32")).toBe(
      true,
    );
    expect(samePathIdentity("\\\\.\\UNC\\server\\share\\x", "\\\\server\\share\\x", "win32")).toBe(
      true,
    );
    expect(samePathIdentity("\\\\?\\UNC\\server\\share\\x", "\\\\server\\share\\x", "linux")).toBe(
      false,
    );
    expect(
      isWithinPathIdentity("\\\\?\\UNC\\server\\share", "\\\\server\\share\\x", "win32"),
    ).toBe(true);
  });

  it("strips trailing separators below the root", () => {
    expect(normalizePathIdentity("C:\\a\\b\\", "win32")).toBe("c:\\a\\b");
    expect(normalizePathIdentity("/a/b/", "linux")).toBe("/a/b");
  });

  it("keeps a UNC root", () => {
    expect(normalizePathIdentity("\\\\server\\share\\", "win32")).toBe("\\\\server\\share");
  });

  it("collapses repeated separators", () => {
    expect(normalizePathIdentity("C:\\a\\\\b", "win32")).toBe("c:\\a\\b");
  });
});

describe("samePathIdentity", () => {
  it("accepts case-only and separator-only differences on Windows", () => {
    expect(samePathIdentity("C:\\Users\\X", "c:/users/x", "win32")).toBe(true);
    expect(samePathIdentity("C:\\Users\\X", "\\\\?\\C:\\Users\\X", "win32")).toBe(true);
  });

  it("rejects case-only differences on Linux", () => {
    expect(samePathIdentity("/a/b", "/A/B", "linux")).toBe(false);
  });

  it("accepts case-only differences on macOS", () => {
    expect(samePathIdentity("/a/b", "/A/B", "darwin")).toBe(true);
  });

  it("rejects different paths", () => {
    expect(samePathIdentity("C:\\a\\b", "C:\\a\\c", "win32")).toBe(false);
    expect(samePathIdentity("/a/b", "/a/c", "linux")).toBe(false);
  });
});

describe("isWithinPathIdentity", () => {
  it("contains direct descendants with a separator boundary", () => {
    expect(isWithinPathIdentity("C:\\foo", "C:\\foo\\bar", "win32")).toBe(true);
    expect(isWithinPathIdentity("C:\\foo", "C:\\foobar", "win32")).toBe(false);
    expect(isWithinPathIdentity("/foo", "/foo/bar", "linux")).toBe(true);
    expect(isWithinPathIdentity("/foo", "/foobar", "linux")).toBe(false);
  });

  it("contains the root itself", () => {
    expect(isWithinPathIdentity("C:\\foo", "C:\\foo", "win32")).toBe(true);
    expect(isWithinPathIdentity("/foo", "/foo", "linux")).toBe(true);
  });

  it("is case-insensitive on Windows and macOS", () => {
    expect(isWithinPathIdentity("C:\\Foo", "c:/foo/bar", "win32")).toBe(true);
    expect(isWithinPathIdentity("/Foo", "/foo/bar", "darwin")).toBe(true);
    expect(isWithinPathIdentity("/Foo", "/foo/bar", "linux")).toBe(false);
  });

  it("contains everything beneath a drive root", () => {
    expect(isWithinPathIdentity("C:\\", "C:\\a\\b", "win32")).toBe(true);
  });

  it("keeps UNC containment bounded to the share", () => {
    expect(isWithinPathIdentity("\\\\server\\share", "\\\\server\\share\\a", "win32")).toBe(true);
    expect(isWithinPathIdentity("\\\\server\\share", "\\\\server\\sharing\\a", "win32")).toBe(
      false,
    );
  });
});
