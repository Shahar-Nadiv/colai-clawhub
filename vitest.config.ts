// The suite runs from this package rather than from the monorepo it was cut out of.
//
// `plugin.test.ts` runs the real launcher script, `toolbar.test.ts` reads source files off
// disk, and `toolbar.page.test.ts` runs the page under linkedom, so none wants a browser
// environment.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["*.test.ts"],
    environment: "node",
    // `toolbar.page.test.ts` boots the whole page per test (every script, the DOM, a few
    // settles). That once took seconds a test, because each settle slept 400ms and each page
    // leaked into the worker's global so the late tests ran slowest, and 20s was needed to
    // keep them passing. Both are gone and the slowest test is a fraction of a second, so
    // this is back near the default: 10s is room for a slow CI box, and a hang shows up as
    // a hang rather than as twenty quiet seconds.
    testTimeout: 10000,
  },
});
