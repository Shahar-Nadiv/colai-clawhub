// The host API this plugin is handed, as much of it as this plugin touches.
//
// It came from `openclaw/plugin-sdk/plugin-test-api` until this package had a repository
// of its own. That helper exists in the OpenClaw checkout and is **not** exported from the
// published `openclaw` package, so the suite ran in one place and nowhere else — which is
// exactly the wrong property for a plugin whose whole review story is "here is the source,
// check it yourself". A reviewer who clones this and runs `npm test` should get the same
// answer the author gets.
//
// Three members, because `index.ts` uses three: `pluginConfig`, `registerService` and
// `registerCli`. Everything else a host offers is deliberately **absent** rather than
// stubbed to a no-op. A no-op for everything is a double that can never fail: the day this
// plugin starts calling a fourth registrar, an enumerated double throws and a permissive
// one quietly does nothing — and "registers nothing, silently" is the exact failure
// `index.test.ts` exists to catch.

import type { OpenClawPluginApi } from "openclaw/plugin-sdk/plugin-runtime";

/** What a caller may fill in; the rest is not there at all. */
export type TestPluginApi = Pick<
  OpenClawPluginApi,
  "id" | "logger" | "pluginConfig" | "registerService" | "registerCli"
>;

export function createTestPluginApi(given: Partial<TestPluginApi>): OpenClawPluginApi {
  const api: TestPluginApi = {
    id: "colai",
    logger: { info() {}, warn() {}, error() {}, debug() {} },
    pluginConfig: {},
    registerService() {},
    registerCli() {},
    ...given,
  } as TestPluginApi;

  /*
   * One cast, at the boundary, with the reason written down.
   *
   * `OpenClawPluginApi` is forty-odd registrars wide and this is five of them. Restating
   * the other thirty-five as no-ops would be a lie in the shape of type-safety — it would
   * type-check and it would hide the thing above. The cast says plainly: this is a
   * partial host, and a plugin that reaches past it will find nothing there.
   */
  return api as OpenClawPluginApi;
}
