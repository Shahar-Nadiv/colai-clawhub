/* The page, actually run.
 *
 * Every other test here reads source and checks what it says. That is why three crashes in a
 * row shipped: `el.flyAutomate` gone from the lookup but left in the flyout list, a bare
 * `showing` resolving to a function in another file, and a rail restored to a coordinate on a
 * monitor that no longer existed. Each was invisible to four hundred passing tests and
 * immediately obvious to anyone who opened the toolbar.
 *
 * So this one opens it. `linkedom` gives a document, the twelve scripts run against it in the
 * order `toolbar.html` loads them, and `start()` is called the way the page calls it. There is
 * no layout and no compositor — geometry is stubbed — so this proves nothing about how
 * anything looks. It proves the page runs, which is the part that kept not being true.
 *
 * What it does NOT reach: anything behind a keystroke. The completion menu had been broken
 * since the first commit and was found by a person pressing `/`, not by a test — and it still
 * would be. Getting the composer on screen here needs more state than opening the work panel,
 * and a test that cannot produce the field it types into is worth less than an honest note
 * saying so. That is the next gap, and it is a real one.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { describe, expect, it as test } from "vitest";

const require = createRequire(import.meta.url);
const { parseHTML } = require("linkedom");
const UI = new URL("./toolbar/ui/", import.meta.url);
const read = (name: string) => readFileSync(new URL(name, UI), "utf8");

const ORDER = ["toolbar-tools.js","toolbar-rail.js","toolbar-mark.js","toolbar-live.js",
  "toolbar-answers.js","toolbar-compose.js","toolbar-library.js","toolbar-work.js",
  "toolbar-toast.js","toolbar-send.js","toolbar-dock.js","toolbar.js"];

/** The page, loaded and started, with every command answering the way a missing one would. */
function openTheToolbar(
  options: {
    remembered?: string;
    screen?: { width: number; height: number };
    /** Commands that answer with something, for the paths a silent `undefined` cannot reach. */
    answers?: Record<string, unknown>;
  } = {},
) {
  const screen = options.screen ?? { width: 1920, height: 1080 };
  const dom = parseHTML(read("toolbar.html"));
  const sandbox: Record<string, unknown> = dom.window;
  const asked: string[] = [];
  const trouble: string[] = [];

  sandbox.window = dom.window;
  sandbox.globalThis = sandbox;
  sandbox.console = { ...console, error: (...a: unknown[]) => trouble.push(a.join(" ")) };
  sandbox.requestAnimationFrame = (fn: () => void) => setTimeout(fn, 0);
  sandbox.cancelAnimationFrame = () => {};
  sandbox.setTimeout = setTimeout;
  sandbox.clearTimeout = clearTimeout;
  sandbox.setInterval = () => 0;
  sandbox.clearInterval = () => {};
  sandbox.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  sandbox.ResizeObserver = class { observe() {} disconnect() {} };
  sandbox.MutationObserver = class { observe() {} disconnect() {} };
  sandbox.getComputedStyle = () => ({ display: "block", visibility: "visible", opacity: "1", getPropertyValue: () => "" });
  sandbox.innerWidth = screen.width;
  sandbox.innerHeight = screen.height;

  // `recall()` reads `window.localStorage`. Setting it on the sandbox alone leaves the real
  // one in place and the page quietly takes its default — which is how a test can "pass"
  // without ever reaching the code it is about.
  const store = {
    getItem: (key: string) => (key === "colai.toolbar.where" ? options.remembered ?? null : null),
    setItem() {}, removeItem() {},
  };
  sandbox.localStorage = store;
  (dom.window as { localStorage: unknown }).localStorage = store;

  const box = { x: 0, y: 0, top: 0, left: 0, right: 100, bottom: 100, width: 100, height: 100 };
  dom.window.Element.prototype.getBoundingClientRect = () => ({ ...box, toJSON: () => box });
  for (const m of ["scrollIntoView", "focus", "blur"]) {
    (dom.window.Element.prototype as Record<string, unknown>)[m] = () => {};
  }
  dom.window.Element.prototype.animate = () => ({ finished: Promise.resolve(), cancel() {} });
  for (const [prop, value] of [["offsetWidth", 100], ["offsetHeight", 30], ["offsetTop", 0], ["offsetLeft", 0]] as const) {
    Object.defineProperty(dom.window.Element.prototype, prop, { get: () => value, configurable: true });
  }

  // Every command answers `undefined` — what the page sees when one is missing, renamed, or
  // not in the permission list. The page must survive all three.
  const desk = [{ x: 0, y: 0, width: screen.width, height: screen.height,
                  reserved: { top: 0, right: 0, bottom: 0, left: 0 } }];
  sandbox.__TAURI__ = {
    core: {
      // Every command answers `undefined` — what the page sees when one is missing, renamed,
      // or not in the permission list, and the page must survive all three. The exception is
      // the screens: without a desk the page correctly says it is guessing at one, and a test
      // that starts in that state is testing the harness rather than the toolbar.
      invoke: async (name: string) => {
        asked.push(name);
        if (options.answers && name in options.answers) return options.answers[name];
        return name === "colai_screens" ? desk : undefined;
      },
    },
    event: { listen: async () => () => {} },
  };

  vm.createContext(sandbox);
  for (const file of ORDER) {
    vm.runInContext(read(file), sandbox, { filename: file });
  }
  const at = () => JSON.parse(vm.runInContext("JSON.stringify(state.at ?? null)", sandbox));
  /*
   * What the toolbar itself says went wrong.
   *
   * `sayFailed` is the page's own catch-all: it sets `state.trouble`, writes the window
   * title, and draws a banner. Every crash reported from the real thing arrived as "The
   * toolbar hit an error — …", so that string is the thing to assert on. Watching for an
   * exception to escape does not work: `start()` catches its own, which is why a page that
   * had entirely failed to draw still looked fine to a test that only waited for a throw.
   */
  const said = () => ({
    trouble: JSON.parse(vm.runInContext("JSON.stringify(state.trouble ?? null)", sandbox)),
    title: String(vm.runInContext("document.title", sandbox)),
  });
  const run = (code: string) => vm.runInContext(code, sandbox);
  return { dom, asked, trouble, at, run, said };
}

const settle = () => new Promise((done) => setTimeout(done, 400));

describe("the page, actually run", () => {
  test("all twelve scripts load and start() completes", async () => {
    const page = openTheToolbar();
    await settle();
    // `start()` runs on load; if it threw, the rail never gets built.
    expect(page.dom.document.querySelector("#rail")?.children.length ?? 0).toBeGreaterThan(10);
    expect(page.dom.document.querySelector(".rail-wrap")?.hidden).toBe(false);
    // And it must not have quietly reported its own failure while drawing something.
    const { trouble, title } = page.said();
    expect(trouble, "the toolbar said it hit an error").toBe(null);
    expect(title, "the title carries the error even when the page cannot draw").not.toContain("error");
  });

  test("render() does not throw", async () => {
    /*
     * Called straight, rather than waited for.
     *
     * `start()` catches its own failures and turns them into a banner, so a page that threw
     * on every redraw still looked like a page that started. Both crashes that shipped were
     * inside `render`: a flyout in the placement list with no element behind it, and a menu
     * mapping over a global function. Calling it here is what makes either one a red test.
     */
    const page = openTheToolbar();
    await settle();
    expect(() => page.run("render()")).not.toThrow();
    expect(() => page.run("render()"), "and again, since a redraw is the common case").not.toThrow();
  });

  test("it asks for the screens before placing anything", async () => {
    const page = openTheToolbar();
    await settle();
    expect(page.asked).toContain("colai_screens");
  });

  test("a rail remembered on a monitor that is gone comes back onto the one that is left", async () => {
    /*
     * The real thing, from a real machine: a position saved when the desk was 3840 wide,
     * restored onto 1920. Half the toolbar off the right edge, the glass still catching the
     * cursor — which reads as a toolbar that is broken rather than one that is elsewhere.
     */
    const page = openTheToolbar({
      remembered: '{"x":2000,"y":345.625,"dock":"left","tucked":true,"away":false}',
      screen: { width: 1920, height: 1080 },
    });
    await settle();
    page.run("state.screens = [{x:0,y:0,width:1920,height:1080,reserved:{top:0,right:0,bottom:0,left:0}}]");
    page.run("recall()");
    expect(page.at().x, "recall must read the remembered position, or this tests nothing").toBe(2000);
    page.run("clamp()");
    const put = page.at();
    expect(put.x, "still off the right edge").toBeLessThan(1920);
    expect(put.x).toBeGreaterThanOrEqual(0);
  });

  test("an unmeasurable rail is still brought back on screen", async () => {
    // `clamp` used to return early when the rail had no width — folded, or not yet laid out —
    // which is exactly when a stranded position most needs pulling back.
    const page = openTheToolbar({ remembered: '{"x":9999,"y":9999,"dock":"left"}' });
    await settle();
    page.run("state.screens = [{x:0,y:0,width:1920,height:1080,reserved:{top:0,right:0,bottom:0,left:0}}]");
    page.run("recall()");
    page.dom.window.Element.prototype.getBoundingClientRect = () =>
      ({ x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) });
    page.run("clamp()");
    expect(page.at().x).toBeLessThan(1920);
    expect(page.at().y).toBeLessThan(1080);
  });

  describe("the conversation the toolbar was opened from", () => {
    const CHATS = [
      { key: "the-one-that-opened-it", title: "Fixing the hotkey", preview: "colai-clawhub", at: 2 },
      { key: "some-other-chat", title: "Something else", preview: "elsewhere", at: 1 },
    ];

    test("it opens pointed at that conversation rather than at a picker", async () => {
      /*
       * `/colai:show` runs inside a chat, and Claude Code puts that chat's id in the
       * environment of everything it spawns — so the toolbar can read the answer to the
       * question the picker was about to ask. Telling the toolbar which conversation you
       * were just in was the most repeated act in using it.
       */
      const page = openTheToolbar({
        answers: { colai_came_from: "the-one-that-opened-it", colai_sessions: CHATS },
      });
      await settle();
      const receiving = JSON.parse(page.run("JSON.stringify(state.receiving)") as string);
      expect(receiving.kind).toBe("session");
      expect(receiving.id).toBe("the-one-that-opened-it");
      expect(receiving.name, "and named, so the rail says which").toBe("Fixing the hotkey");
    });

    test("a toolbar started from a terminal belongs to no conversation", async () => {
      // Which is the honest answer, and the picker is the right thing to show for it.
      const page = openTheToolbar({ answers: { colai_sessions: CHATS } });
      await settle();
      const receiving = JSON.parse(page.run("JSON.stringify(state.receiving)") as string);
      expect(receiving.kind).not.toBe("session");
    });

    test("a later /colai:show from another chat moves it", async () => {
      /*
       * The event, which is the only route once the page is already up: the second launch
       * hands its arguments to the copy on screen and then exits, so nothing is left for
       * the page to ask.
       *
       * This overrules a choice somebody made in the menu, deliberately. Running
       * `/colai:show` inside a conversation is as clear a statement of where the next mark
       * should go as picking from the list is, and it is the more recent of the two.
       */
      const page = openTheToolbar({
        answers: { colai_came_from: "the-one-that-opened-it", colai_sessions: CHATS },
      });
      await settle();
      page.run('heardWhichChat("some-other-chat")');
      const receiving = JSON.parse(page.run("JSON.stringify(state.receiving)") as string);
      expect(receiving.id).toBe("some-other-chat");
      expect(receiving.name).toBe("Something else");
    });

    test("a conversation too new to have a transcript is waited for, not invented", async () => {
      /*
       * Claude Code writes the transcript, and the session list is read from those files —
       * so a chat that has not said anything yet can be the one that launched the toolbar
       * and still have nothing on disk. Putting a made-up row in the rail would offer a
       * receiver that cannot be sent to; asking the list again is what actually helps.
       */
      const page = openTheToolbar({
        answers: { colai_came_from: "not-on-disk-yet", colai_sessions: CHATS },
      });
      await settle();
      const receiving = JSON.parse(page.run("JSON.stringify(state.receiving)") as string);
      expect(receiving.id).not.toBe("not-on-disk-yet");
      const { trouble } = page.said();
      expect(trouble, "and it must not be an error").toBe(null);
    });
  });

  describe("colai's own mark", () => {
    test("the jellyfish is on the rail, and its eyes are holes", async () => {
      /*
       * Three marks have stood on this key — OpenClaw's crab, a C from the old wordmark,
       * and now the logo colai actually has. Checked by rendering rather than by reading
       * the source, because what matters is that it reaches the DOM: the C shipped once
       * with a stale element in the flyout list and threw on every render.
       *
       * The eyes are punched out of a masked fill rather than painted, so a hole shows
       * whatever is behind the rail. Filled eyes in a fixed colour would be right on a dark
       * desktop and invisible on a light one.
       */
      const page = openTheToolbar();
      await settle();
      const home = page.dom.document.querySelector(".home-key svg");
      expect(home, "the home key must carry a mark").toBeTruthy();
      // Two masked fills: the tentacles and the dome. Both take the rail's own colour.
      expect(home?.querySelectorAll('rect[fill="currentColor"]').length).toBe(2);
      expect(home?.querySelectorAll("mask").length, "eyes are a mask, not paint").toBe(2);
      // The dome's mask paints the eyes black — that is what makes them holes.
      expect(home?.innerHTML).toContain('fill="#000"');
    });

    test("only the tentacles move, and each mark masks itself", async () => {
      const page = openTheToolbar();
      await settle();
      const home = page.dom.document.querySelector(".home-key svg");
      // The dome holds still so the mark stays a mark at seventeen pixels rather than
      // becoming a spinner that happens to be green.
      expect(home?.querySelectorAll(".colai-arms").length).toBe(1);

      /*
       * Mask ids carry the size because two marks can be on screen at once — the rail's at
       * seventeen pixels and the pin waiting on a reply at fourteen. Duplicate ids in one
       * document mean the second mark is masked by the first one's shape, which is a mark
       * with a jellyfish-shaped hole in it.
       */
      const ids = [...(home?.querySelectorAll("mask") ?? [])].map((one) =>
        (one as unknown as { id: string }).id,
      );
      expect(new Set(ids).size, "the two masks must not share an id").toBe(2);
      for (const id of ids) {
        expect(id, `${id} must be scoped to its size`).toMatch(/-\d+$/);
      }
    });
  });
});
