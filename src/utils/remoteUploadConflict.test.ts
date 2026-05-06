import { describe, expect, it } from "vitest";
import type { FileEntry } from "../types";
import { remoteDirListsFileNamed } from "./remoteUploadConflict";

describe("remoteDirListsFileNamed", () => {
  it("空列表返回 false", () => {
    expect(remoteDirListsFileNamed([], "a.txt")).toBe(false);
  });

  it("存在同名文件返回 true", () => {
    const files: FileEntry[] = [
      {
        name: "a.txt",
        path: "/home/a.txt",
        is_dir: false,
        size: 1,
      },
    ];
    expect(remoteDirListsFileNamed(files, "a.txt")).toBe(true);
  });

  it("仅同名目录不视为占用", () => {
    const files: FileEntry[] = [
      {
        name: "a.txt",
        path: "/home/a.txt",
        is_dir: true,
        size: 0,
      },
    ];
    expect(remoteDirListsFileNamed(files, "a.txt")).toBe(false);
  });

  it("其它文件名不匹配", () => {
    const files: FileEntry[] = [
      {
        name: "b.txt",
        path: "/home/b.txt",
        is_dir: false,
        size: 1,
      },
    ];
    expect(remoteDirListsFileNamed(files, "a.txt")).toBe(false);
  });
});
