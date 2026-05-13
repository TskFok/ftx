import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

describe("App.css", () => {
  it("禁用全局滚动边界回弹", () => {
    const css = readFileSync(join(here, "App.css"), "utf8");
    expect(css).toMatch(/overscroll-behavior:\s*none/);
  });
});
