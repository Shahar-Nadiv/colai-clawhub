// The suite runs from this package rather than from the monorepo it was cut out of.
//
// `index.test.ts` spawns the real binary and `toolbar.test.ts` reads two source files off
// disk, so neither wants a browser environment.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["*.test.ts"],
    environment: "node",
  },
});
