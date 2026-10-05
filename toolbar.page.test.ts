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
 * It reaches keystrokes now, which it did not. That was the gap named here: the completion
 * menu had been broken since the first commit and was found by a person pressing `/` rather
 * than by any of four hundred tests, because none of them could produce the field they would
 * have typed into. So the marks, the answers and Claude Code's command list are put into
 * `state` the way the page's own frames put them there, a real `input` or `keydown` is
 * dispatched into a real field, and what got drawn is read back out of the DOM.
 *
 * What it still does NOT reach is layout. There is no layout engine here, so nothing in this
 * file can prove that one box sits inside another — which is a class of bug only a person
 * looking at the screen catches. Where a test here is about a size or a position it says so,
 * and checks the cause rather than claiming to have seen the effect.
 */
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import vm from "node:vm";
import { describe, expect, it as test } from "vitest";

const require = createRequire(import.meta.url);
const { parseHTML } = require("linkedom");
const UI = new URL("./toolbar/ui/", import.meta.url);
const read = (name: string) => readFileSync(new URL(name, UI), "utf8");

const ORDER = ["toolbar-tools.js","toolbar-markdown.js","toolbar-jelly.js","toolbar-rail.js","toolbar-mark.js","toolbar-live.js",
  "toolbar-answers.js","toolbar-compose.js","toolbar-work.js",
  "toolbar-toast.js","toolbar-send.js","toolbar-dock.js","toolbar-shell.js","toolbar-context.js","toolbar-settings.js","toolbar-schedule.js","toolbar-response.js","toolbar.js"];

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
  /*
   * The page's own global, kept to this page.
   *
   * `dom.window` is not a window: linkedom hands back a Proxy over Node's `globalThis`, so
   * contextifying it directly meant every `function` the scripts declared, every stub below
   * and every `state` they built was written into the test worker's own global — and the
   * next page booted on top of the last one's leftovers. Each test ran a little slower than
   * the one before it, and could read what an earlier one had left behind.
   *
   * So writes land here, on an object this page alone owns, and only a read that misses
   * falls through to `dom.window` — which is still where `document`, `Event`, `Element` and
   * the window's event target come from.
   */
  const own: Record<string | symbol, unknown> = {};
  const sandbox = new Proxy(own, {
    get: (target, name) => (name in target ? target[name] : Reflect.get(dom.window, name)),
    has: (target, name) => name in target || name in dom.window,
  }) as Record<string, unknown>;
  const asked: string[] = [];
  // The same calls, with their arguments — for tests that care which prompt a rewind names,
  // not only that a rewind was asked for.
  const askedWith: [string, unknown][] = [];
  const trouble: string[] = [];
  // The page's event listeners, by event name — see `event.listen` below.
  const heard: Record<string, (event: unknown) => void> = {};

  sandbox.window = sandbox;
  sandbox.globalThis = sandbox;
  sandbox.console = { ...console, error: (...a: unknown[]) => trouble.push(a.join(" ")) };
  sandbox.requestAnimationFrame = (fn: () => void) => setTimeout(fn, 0);
  sandbox.cancelAnimationFrame = () => {};
  sandbox.setTimeout = setTimeout;
  sandbox.clearTimeout = clearTimeout;
  // Intervals are recorded, never fired on their own: a test that cares about one (the
  // jellyfish loop) finds it here by id and ticks it by hand.
  const intervals = new Map<number, { fn: () => void; ms: number }>();
  let nextInterval = 1;
  sandbox.setInterval = (fn: () => void, ms: number) => {
    const id = nextInterval++;
    intervals.set(id, { fn, ms });
    return id;
  };
  sandbox.clearInterval = (id: number) => {
    intervals.delete(id);
  };
  sandbox.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  sandbox.ResizeObserver = class { observe() {} disconnect() {} };
  sandbox.MutationObserver = class { observe() {} disconnect() {} };
  sandbox.getComputedStyle = () => ({ display: "block", visibility: "visible", opacity: "1", getPropertyValue: () => "" });
  // `CSS.escape` is a browser global linkedom does not carry, and `rowFor` reaches for it the
  // moment a send scrolls its own row into view. Stubbed like the rest of the geometry — a
  // faithful-enough escape for the simple session keys these tests use.
  sandbox.CSS = { escape: (value: string) => String(value).replace(/["\\]/g, "\\$&") };
  sandbox.innerWidth = screen.width;
  sandbox.innerHeight = screen.height;

  // `recall()` reads `window.localStorage`, and `window` is this sandbox now, so setting it
  // here is the whole of it. (While `window` was linkedom's, setting it on the sandbox alone
  // left the real one in place and the page quietly took its default — which is how a test
  // can "pass" without ever reaching the code it is about.)
  const store = {
    getItem: (key: string) => (key === "colai.toolbar.where" ? options.remembered ?? null : null),
    setItem() {}, removeItem() {},
  };
  sandbox.localStorage = store;

  const box = { x: 0, y: 0, top: 0, left: 0, right: 100, bottom: 100, width: 100, height: 100 };
  dom.window.Element.prototype.getBoundingClientRect = () => ({ ...box, toJSON: () => box });
  for (const m of ["scrollIntoView", "focus", "blur"]) {
    (dom.window.Element.prototype as Record<string, unknown>)[m] = () => {};
  }
  dom.window.Element.prototype.animate = () => ({ finished: Promise.resolve(), cancel() {} });
  /*
   * A caret, which linkedom has no concept of.
   *
   * Its textareas hold a `value` and nothing else — no `selectionStart`, no
   * `setSelectionRange` — and the page needs both: every `/` and `@` menu asks where the
   * caret is before it decides what is being typed, and puts it back after completing a
   * word. Stubbed the way the geometry above is, and it records rather than discards, so a
   * test can ask where the caret was left.
   */
  (dom.window.Element.prototype as Record<string, unknown>).setSelectionRange = function (
    this: { selectionStart: number; selectionEnd: number },
    from: number,
    to: number,
  ) {
    this.selectionStart = from;
    this.selectionEnd = to;
  };
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
      invoke: async (name: string, args?: unknown) => {
        asked.push(name);
        askedWith.push([name, args]);
        if (options.answers && name in options.answers) {
          // An `Error` in the map means that one command refuses, which is a third thing a
          // command can do and is not the same as answering with nothing. Several of these
          // go out together, and what the page does with one refusal among four answers is
          // its own behaviour.
          const answer = options.answers[name];
          if (answer instanceof Error) throw answer;
          return answer;
        }
        return name === "colai_screens" ? desk : undefined;
      },
    },
    // Kept rather than dropped, so a test can stand in for a frame the Rust side would emit
    // and reach the page's own listener — nothing calls them unless a test does.
    event: {
      listen: async (name: string, fn: (event: unknown) => void) => {
        heard[name] = fn;
        return () => {};
      },
    },
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

  /** The box named `data-field`, which is how the page finds them itself. */
  type Box = {
    value: string;
    selectionStart: number;
    dispatchEvent: (event: unknown) => void;
    parentElement: { querySelector: (what: string) => Element | null };
  };
  const named_ = (named: string) =>
    dom.document.querySelector(`[data-field="${named}"]`) as unknown as Box | null;

  /*
   * Typing, as far as anything without a keyboard can go.
   *
   * The caret goes to the end of what was typed and then `input` is dispatched, which is the
   * pair of facts every completion menu reads. This is the thing the twelve scripts had
   * never been asked to do: the menu was broken from the first commit and was found by a
   * person pressing `/`, because no test here could produce a keystroke.
   */
  const type = (named: string, said: string) => {
    const field = named_(named);
    if (!field) throw new Error(`no field named ${named}`);
    field.value = said;
    field.selectionStart = said.length;
    field.dispatchEvent(new dom.window.Event("input"));
    return field;
  };

  /** One key, in the box that has the caret. `keydown` is where the menus are driven. */
  const press = (named: string, key: string, how: Record<string, unknown> = {}) => {
    const field = named_(named);
    if (!field) throw new Error(`no field named ${named}`);
    const event = new dom.window.Event("keydown") as unknown as Record<string, unknown>;
    event.key = key;
    Object.assign(event, how);
    field.dispatchEvent(event);
  };

  /** The menu belonging to one field, and the rows in it. */
  const menu = (named: string) => {
    const field = named_(named);
    const found = field ? field.parentElement.querySelector(".ask-menu") : null;
    const rows = found ? [...found.querySelectorAll(".ask-menu-row")] : [];
    return {
      showing: found ? !(found as unknown as { hidden: boolean }).hidden : false,
      names: rows.map((row) => row.querySelector(".ask-menu-name")?.textContent ?? ""),
      says: rows.map((row) => row.querySelector(".ask-menu-says")?.textContent ?? ""),
      rows,
      under: found ? (found as HTMLElement).dataset.under : null,
    };
  };

  return { dom, asked, askedWith, heard, trouble, at, run, said, box: named_, type, press, menu, intervals };
}

/*
 * Let the page finish what it started.
 *
 * Every command answers at once, every `requestAnimationFrame` is a `setTimeout(0)` and
 * intervals never fire on their own, so what is left after a boot or a click is a short
 * chain of promise jobs and zero-delay timers. A few macrotask turns run all of it. A fixed
 * 400ms sleep ran it too, a hundred and forty times over, and was most of this file's minute
 * and a half.
 *
 * Two timer turns, because a frame queued from inside a frame needs a second one; one is
 * enough for every test today, so the second is margin. Then a few `setImmediate` turns for
 * any promise chain still going — those are not clamped to the timer tick the way a
 * `setTimeout(0)` is on Windows, where each one costs several milliseconds.
 *
 * Nothing here waits on a real delay, and nothing needs to: the only page timers longer
 * than zero are the 5s toast and receipt fades, which the old 400ms never reached either.
 * A test that comes to depend on one should wait for it by name rather than lengthen this.
 */
const settle = async () => {
  for (let turn = 0; turn < 2; turn++) await new Promise((done) => setTimeout(done, 0));
  for (let turn = 0; turn < 3; turn++) await new Promise((done) => setImmediate(done));
};

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
    test("the jellyfish is on the rail, drawn with swimming tentacles and a face", async () => {
      // The mark is the ad's living jellyfish (toolbar-jelly.js), built as SVG DOM: five procedural
      // tentacles plus the body (six paths), two eyes and two pupils. A first frame is painted at
      // build, so the tentacles have geometry even before the rAF loop runs.
      const page = openTheToolbar();
      await settle();
      const home = page.dom.document.querySelector(".home-key svg");
      expect(home, "the home key must carry a mark").toBeTruthy();
      expect(home?.querySelectorAll("path").length, "five tentacles plus the dome").toBe(6);
      expect(home?.querySelectorAll("ellipse").length, "two eyes").toBe(2);
      expect(home?.querySelectorAll("circle").length, "two pupils").toBe(2);
      const tentacle = home?.querySelector("g path");
      expect((tentacle?.getAttribute("d") || "").length, "a tentacle is actually drawn").toBeGreaterThan(0);
      expect(home?.innerHTML, "never repainted by currentColor").not.toContain("currentColor");
    });

    test("at rest the mark is the calm grey, not the working green", async () => {
      const page = openTheToolbar();
      await settle();
      const home = page.dom.document.querySelector(".home-key svg");
      const fills = [...(home?.querySelectorAll("path") ?? [])]
        .map((p) => p.getAttribute("fill"))
        .filter(Boolean);
      expect(fills, "the body is grey at rest").toContain("#b9b9be");
      expect(fills, "not green while nothing runs").not.toContain("#22c55e");
    });

    /** The jellyfish loop's own interval, as the page armed it. */
    const jellyLoop = (page: ReturnType<typeof openTheToolbar>) => {
      const id = page.run("jellyTimer") as number;
      const found = page.intervals.get(id);
      expect(found, "the jellyfish loop is armed").toBeTruthy();
      return found!;
    };

    test("the swim runs at ~30fps or slower, never 60, idle or working", async () => {
      // Repainting the SVG at 60fps burned a quarter of a core on an idle toolbar.
      const page = openTheToolbar();
      await settle();
      page.run(`state.atWork = null; state.runs = []; render();`);
      jellyLoop(page).fn();
      expect(jellyLoop(page).ms, "idle period").toBeGreaterThanOrEqual(30);
      page.run(`
        state.atWork = null;
        state.runs = [{ sessionKey: "s1", who: "Claude", heard: Date.now() }];
        render();
      `);
      jellyLoop(page).fn();
      expect(page.run("jellyWorking()"), "the agent is working").toBe(true);
      expect(jellyLoop(page).ms, "working period").toBeGreaterThanOrEqual(30);
      expect(jellyLoop(page).ms, "still lively while working").toBeLessThanOrEqual(40);    });

    test("it stops painting while the rail is put away, and swims again when it returns", async () => {
      const page = openTheToolbar();
      await settle();
      const frames = () => page.run("jellyFrames") as number;

      let before = frames();
      jellyLoop(page).fn();
      expect(frames(), "swimming while on screen").toBeGreaterThan(before);

      page.run("state.away = true;");
      jellyLoop(page).fn();
      before = frames();
      for (let i = 0; i < 5; i++) jellyLoop(page).fn();
      expect(frames(), "no frames while put away").toBe(before);
      expect(jellyLoop(page).ms, "only a slow watch while put away").toBeGreaterThanOrEqual(400);

      page.run("state.away = false;");
      jellyLoop(page).fn();
      expect(frames(), "the watch resumes it").toBeGreaterThan(before);
      expect(jellyLoop(page).ms, "back at swimming pace").toBeLessThan(100);
      before = frames();
      jellyLoop(page).fn();
      expect(frames(), "and keeps swimming").toBeGreaterThan(before);
    });

    test("unfolding the rail resumes the swim on that render, not half a second later", async () => {
      const page = openTheToolbar();
      await settle();
      page.run("state.away = true;");
      jellyLoop(page).fn();
      const before = page.run("jellyFrames") as number;
      page.run("state.away = false; render();");
      expect(page.run("jellyFrames"), "render woke it").toBeGreaterThan(before);
      expect(jellyLoop(page).ms).toBeLessThan(100);
    });

    test("a hidden window paints nothing", async () => {
      const page = openTheToolbar();
      await settle();
      page.run(`Object.defineProperty(document, "hidden", { value: true, configurable: true });`);
      jellyLoop(page).fn();
      const before = page.run("jellyFrames") as number;
      jellyLoop(page).fn();
      expect(page.run("jellyFrames")).toBe(before);
      page.run(`Object.defineProperty(document, "hidden", { value: false, configurable: true });`);
      jellyLoop(page).fn();
      expect(page.run("jellyFrames")).toBeGreaterThan(before);
    });
  });
});

/*
 * ── the keystrokes, actually pressed ─────────────────────────────────────────
 *
 * The note at the top of this file said the completion menu was the gap: "anything behind a
 * keystroke" was out of reach, `/` had been broken since the first commit, and a test that
 * cannot produce the field it types into is worth less than an honest note saying so.
 *
 * This is that gap closed. The marks, the answers and the commands are put into `state` the
 * way the page's own frames put them there, a real `input` event is dispatched into a real
 * field, and what the menu drew is read back out of the DOM.
 */

/** Claude Code's own list, as it arrives on the init frame. `/clear` belongs to a terminal. */
const COMMANDS = ["review", "commit", "compact", "clear"];

/** A mark on screen with its popup open, which is the toolbar's headline act. */
const MARKED = `
  state.marks.push({
    id: "mark-1", tool: "box", note: "", chosen: true,
    points: [{ x: 0.2, y: 0.2 }, { x: 0.4, y: 0.4 }], where: null,
  });
  state.popup = "mark-1";
  state.commands = ${JSON.stringify(COMMANDS)};
  state.terminalOnly = ["clear"];
  render();
`;

/** An agent that has stopped to ask something, which is what both reply boxes are for. */
const ASKING = `
  state.answers = [{
    sessionKey: "chat-1", who: "Claude", heard: 2, saying_text: "",
    turns: [{ said: "Which of the two should I change?", mine: false }],
  }];
  state.history = [{
    at: 2, who: "Claude", sessionKey: "chat-1", said: "have a look",
    count: 0, answer: state.answers[0], view: { open: true, shown: new Set() },
  }];
  state.commands = ${JSON.stringify(COMMANDS)};
  state.terminalOnly = ["clear"];
  state.work.scope = "all";
  render();
`;

describe("what `/` actually offers", () => {
  test("Claude Code's own commands arrive without a conversation having happened", async () => {
    /*
     * Reported as "the / sign allows only 4 modes, where in claude chat there are a lot
     * more options" — and it was exactly that: four, on a machine with a hundred and one.
     *
     * The list is filled from the `system/init` preamble of a `claude` the toolbar started,
     * and that used to be the only source. It worked while every mark ran through the
     * toolbar's own agent; it stopped the moment marks began going straight into live
     * conversations, because then no such agent ever starts. So the menu fell back to
     * colai's four modes and stayed there.
     *
     * The toolbar asks at startup now, on its own, and `colai:commands` is the answer.
     * This is the guard that the page actually takes it: without the listener the modes
     * come back, and nobody would notice until somebody counted.
     */
    const page = openTheToolbar();
    await settle();

    // Before the answer arrives there is nothing to offer but the modes — which is the
    // right fallback, and is also the bug's signature.
    expect(page.run("JSON.stringify(state.commands)")).toBe("[]");

    page.run(`
      heardCommands({
        slashCommands: ["review", "commit", "compact", "model", "usage", "doctor"],
        terminalOnly: ["doctor"],
      });
    `);
    const known = JSON.parse(page.run("JSON.stringify(state.commands)") as string);
    expect(known.length, "the page must keep what Claude Code named").toBe(6);
    expect(JSON.parse(page.run("JSON.stringify(state.terminalOnly)") as string)).toEqual([
      "doctor",
    ]);
  });

  test("an empty answer leaves the modes in place rather than emptying the menu", async () => {
    /*
     * A `claude` that could not start, or a frame whose field was renamed, gives nothing.
     * Taking that as the answer would replace a short menu with no menu — and `/` would
     * stop working entirely rather than offering less than it could.
     */
    const page = openTheToolbar();
    await settle();
    page.run('heardCommands({ slashCommands: ["review"], terminalOnly: [] })');
    page.run("heardCommands({ slashCommands: [], terminalOnly: [] })");
    expect(JSON.parse(page.run("JSON.stringify(state.commands)") as string)).toEqual(["review"]);
  });
});

describe("the same keystrokes in every box there is to type in", () => {
  test("the note on a mark answers `/` with Claude Code's own commands", async () => {
    /*
     * The one the user asked for, and the one that was missing.
     *
     * Marking something on screen and saying what you want about it is the whole of what
     * colai is for, and this box — the one that opens by itself on the thing you just
     * pointed at — was the only field of the four where `/` did nothing at all. Typed for
     * real rather than read out of the source, because reading the source is exactly how
     * this stayed broken.
     */
    const page = openTheToolbar();
    await settle();
    page.run(MARKED);
    page.type("popup-note:mark-1", "/");

    const shown = page.menu("popup-note:mark-1");
    expect(shown.showing, "the menu must open in the popup").toBe(true);
    expect(shown.names, "and it must be Claude Code's real list").toContain("/review");
    expect(shown.names).toContain("/commit");
    // Not colai's four modes standing in: those are the fallback for a conversation that
    // has not started, and this one has a list.
    expect(shown.names, "the modes must not narrow the real list").not.toContain("Build");
    // And never a command only a terminal can run, from a rail that has no terminal.
    expect(shown.names, "/clear does nothing from here").not.toContain("/clear");
    expect(page.said().trouble, "and nothing may have fallen over").toBe(null);
  });

  test("before a conversation has begun it answers with the modes instead", async () => {
    // The fallback the composer has always had, on the popup's field too: a `/` that
    // answers with nothing is worse than one that answers with what colai can still do.
    const page = openTheToolbar();
    await settle();
    page.run(MARKED.replace(JSON.stringify(COMMANDS), "[]"));
    page.type("popup-note:mark-1", "/");
    expect(page.menu("popup-note:mark-1").names).toEqual(["Ask", "Plan", "Debug", "Build"]);
  });

  test("picking a mode from a mark's note sets it, and takes the word back out", async () => {
    // The mode is how the ask is read rather than part of it, and the popup and the
    // composer share one — so choosing it from either is choosing it for the send.
    const page = openTheToolbar();
    await settle();
    page.run(MARKED.replace(JSON.stringify(COMMANDS), "[]"));
    page.type("popup-note:mark-1", "this bit /de");
    page.press("popup-note:mark-1", "Enter");
    expect(page.run("state.mode")).toBe("debug");
    expect(page.run("state.marks[0].note"), "and the word is gone").toBe("this bit ");
  });

  test("picking a command leaves the command in the words", async () => {
    /*
     * Because colai has nothing to set for it and Claude Code is the thing that reads it.
     *
     * It used to be written into `state.mode`, which holds one of four modes — so picking
     * `/review` left the mode unreadable, the message said "Plan" instead, and the command
     * was never sent at all. Silent, and only visible by reading the message afterwards.
     */
    const page = openTheToolbar();
    await settle();
    page.run(MARKED);
    const field = page.type("popup-note:mark-1", "/rev");
    page.press("popup-note:mark-1", "Enter");
    expect(page.run("state.marks[0].note")).toBe("/review ");
    expect(page.run("state.mode"), "and the mode is left alone").toBe("build");
    /*
     * And the caret lands after the space, ready for the rest of the sentence.
     *
     * Read off the box that was typed into rather than the one on screen now: completing a
     * word redraws the panel, and the caret is deliberately set before that happens because
     * the redraw is the thing that reads it. It used to be set afterwards, on a box that had
     * already been replaced, which put the cursor precisely nowhere.
     */
    expect(field.selectionStart).toBe(8);
  });

  test("`@` in a mark's note brings a file along", async () => {
    // The other keystroke, and the one named nowhere else in the toolbar: a path chosen
    // here is described by the machine and carried the same way a dropped file is.
    const page = openTheToolbar({
      answers: {
        colai_search_files: [{ path: "src/rail.js", shown: "rail.js" }],
        colai_describe_files: {
          chosen: [{ path: "src/rail.js", name: "rail.js", bytes: 40, carried: true }],
          refused: [],
        },
      },
    });
    await settle();
    page.run(MARKED);
    page.type("popup-note:mark-1", "look at @rai");
    // The list comes off the disk, so it arrives a tick after the keystroke that wanted it.
    await settle();
    expect(page.menu("popup-note:mark-1").names).toEqual(["rail.js"]);
    page.press("popup-note:mark-1", "Enter");
    await settle();
    expect(page.run("state.files.map((one) => one.path)")).toEqual(["src/rail.js"]);
    expect(page.run("state.marks[0].note"), "the word goes; the file travels").toBe("look at ");
  });

  test("the `@` file search tells Rust which conversation it is typed into", async () => {
    /*
     * Rust derives the search roots from the session, not from the caller — a page that could
     * name its own roots could name `/`. So the invoke has to carry the receiver's id, and the
     * bug was sending only the query: the roots came back empty and the picker found nothing,
     * so the `@` list never appeared.
     */
    const page = openTheToolbar({
      answers: { colai_search_files: [{ path: "src/rail.js", shown: "rail.js" }] },
    });
    await settle();
    page.run(`state.receiving = { id: "s1", name: "My session" }; state.work.open = true; render()`);
    page.type("ask", "look at @rai");
    // The list comes off the disk, so it arrives a tick after the keystroke that wanted it.
    await settle();
    expect(
      page.askedWith.find(([n]) => n === "colai_search_files")?.[1],
      "the search carries the session it is typed into",
    ).toMatchObject({ query: "rai", sessionKey: "s1" });
  });

  test("the reply to an agent's question answers `/` as well", async () => {
    /*
     * This box opens by itself while an agent is stopped and waiting, which makes it the
     * one somebody is most likely to be typing in — and its words go to the conversation
     * exactly as typed, so a command in it is a command that actually runs.
     */
    const page = openTheToolbar();
    await settle();
    page.run(ASKING);
    // The question popup, rather than the Work panel's copy of the same reply.
    expect(page.dom.document.querySelector("#fly-ask")?.hidden).toBe(false);
    const named = "say:chat-1";
    expect(page.dom.document.querySelector(`#fly-ask [data-field="${named}"]`)).toBeTruthy();
    page.type(named, "/rev");
    expect(page.menu(named).names).toContain("/review");
    page.press(named, "Enter");
    // Left in the words, because the words are the whole message here.
    expect(page.run("state.answers[0].saying_text")).toBe("/review ");
  });

  test("and so does the reply beside an answer in the Work panel", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(ASKING);
    // With the question popup waved away, the panel's own reply box is the one on screen.
    page.run("hideAsked(); state.work.open = true; render()");
    const field = page.dom.document.querySelector('.work-answer [data-field="say:chat-1"]');
    expect(field, "the Work panel must have its own reply box").toBeTruthy();
    page.type("say:chat-1", "@rail");
    expect(page.said().trouble).toBe(null);
    page.type("say:chat-1", "/co");
    expect(page.menu("say:chat-1").names).toEqual(["/commit", "/compact"]);
  });

  test("the composer still has the menu it always had", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.commands = ${JSON.stringify(COMMANDS)}; state.terminalOnly = ["clear"];
              state.work.open = true; render()`);
    page.type("ask", "/rev");
    expect(page.menu("ask").names).toContain("/review");
    page.press("ask", "Enter");
    expect(page.run("state.text")).toBe("/review ");
  });

  test("two boxes open at once do not share a highlighted row", async () => {
    /*
     * A mark's popup opens over the Work panel, so two of these lists can be on screen
     * together. One `state.ask` between them meant one highlighted row: an arrow pressed in
     * the box being looked at moved the choice in the box that was not, and Enter then took
     * a row nobody had pointed at.
     */
    const page = openTheToolbar();
    await settle();
    page.run(`${MARKED} state.work.open = true; render()`);
    page.type("ask", "/");
    page.type("popup-note:mark-1", "/");
    page.press("popup-note:mark-1", "ArrowDown");
    page.press("popup-note:mark-1", "ArrowDown");
    expect(page.run("state.marks[0].asking.picked"), "the box being typed in moved").toBe(2);
    expect(page.run("state.ask.picked"), "the other one did not").toBe(0);
  });

  test("an open list survives a redraw it has nothing to do with", async () => {
    // The five-second refresh, a reply arriving, a window moving under the toolbar: all of
    // them rebuild the field. The list has to still be there afterwards, and drawn.
    const page = openTheToolbar();
    await settle();
    page.run(MARKED);
    page.type("popup-note:mark-1", "/co");
    page.run("render()");
    const shown = page.menu("popup-note:mark-1");
    expect(shown.showing, "still open after a redraw").toBe(true);
    expect(shown.names).toEqual(["/commit", "/compact"]);
  });

  test("Escape closes the list without throwing the mark away", async () => {
    // Escape in a popup discards the mark, which is what the window does with it. A
    // suggestion list has to swallow the first press or a menu somebody opened by accident
    // takes their mark with it.
    const page = openTheToolbar();
    await settle();
    page.run(MARKED);
    page.type("popup-note:mark-1", "/co");
    page.press("popup-note:mark-1", "Escape");
    expect(page.menu("popup-note:mark-1").showing).toBe(false);
    expect(page.run("state.marks.length"), "the mark is still here").toBe(1);
  });

  test("the menu opens away from the edge it is against", async () => {
    // Upward is right for the composer, whose field is the last thing in a tall panel, and
    // wrong for a popup whose note is near the top of a card that can be anywhere on the
    // screen. Measured off the field rather than fixed per surface.
    const page = openTheToolbar();
    await settle();
    page.run(MARKED);
    page.type("popup-note:mark-1", "/");
    // Every box in this harness measures as being at the top of the window, so every menu
    // here opens downward. What is under test is that the direction is decided at all.
    expect(page.menu("popup-note:mark-1").under).toBe("true");
  });
});

describe("an ask that fails, and what is kept when it does", () => {
  const CHAT = { key: "chat-1", title: "Fixing the hotkey", preview: "colai-clawhub", at: 2 };

  test("a conversation list that fails is said out loud", async () => {
    /*
     * The reason the banner exists: this really is news about the list, and replacing the
     * list with it is a fair trade.
     *
     * There used to be four other asks beside it under one `Promise.all` and one `catch`,
     * so a rejection from any of them — a missing component catalogue, most memorably —
     * threw away the conversations that had arrived and put this banner over a list that
     * was perfectly good. Those four are gone; the one ask left is the one the banner is
     * genuinely about.
     */
    const page = openTheToolbar({
      answers: { colai_sessions: new Error("the socket is down") },
    });
    await settle();
    expect(page.run("state.whoTrouble")).toBe("the socket is down");
    page.run('state.open = "chat"; render()');
    expect(page.dom.document.querySelector("#chat-rows")?.textContent ?? "").toContain(
      "Could not reach the Gateway",
    );
  });

  test("what came back last time is kept rather than emptied", async () => {
    // "Nobody there" and "could not ask" are different facts, and emptying a list is how
    // the second gets told as the first.
    const page = openTheToolbar({ answers: { colai_sessions: [CHAT] } });
    await settle();
    expect(page.run("state.sessions.length")).toBe(1);
    page.run(`__TAURI__.core.invoke = async (name) => {
      if (name === "colai_sessions") throw new Error("the socket blinked");
      return undefined;
    }`);
    page.run("loadWho()");
    await settle();
    expect(page.run("state.sessions.length"), "the conversation is still known").toBe(1);
  });
});

describe("colai's key opens plugin settings", () => {
  test("the jellyfish key opens a settings panel on the shell, with toggles and how-to", async () => {
    const page = openTheToolbar();
    await settle();
    // The old first-run card is gone entirely.
    expect(page.dom.document.querySelector("#tips"), "no tips card").toBeNull();
    expect(page.run("typeof drawTips"), "and no drawTips").toBe("undefined");

    const home = page.dom.document.querySelector(".home-key") as unknown as {
      dispatchEvent: (event: unknown) => void;
      title: string;
    };
    home.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.panel"), "the mark opens settings on the shell").toBe("settings");
    const panel = page.dom.document.querySelector("#settings");
    expect(panel?.hidden).toBe(false);
    expect(home.title, "the key says what it does").toMatch(/settings/i);

    // The how-to section keeps the four hints the card used to carry.
    const said = panel?.textContent ?? "";
    for (const named of ["Drag the grip", "fold key", "Type / in any box", "Type @ in any box"]) {
      expect(said, `settings should still teach: ${named}`).toContain(named);
    }
    // And the toggle that configures the plugin is there. The Context map toggle is gone —
    // that feature is dormant behind a flag now, with no UI to re-enable it — so Review is
    // the one that remains.
    expect(panel?.querySelectorAll(".set-toggle").length, "the Review toggle").toBeGreaterThanOrEqual(1);
    expect(panel?.textContent ?? "", "and no Context map toggle to re-enable it").not.toContain(
      "Context map",
    );

    // A second press closes it.
    home.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.panel"), "and the same press closes it").toBe(null);
  });

  test("the permission default is a persisted setting applied to the session", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSettings()");
    page.run(`(() => {
      const row = [...document.querySelectorAll('#settings .set-row')].find((r) => r.textContent.includes("without asking"));
      const ask = [...row.querySelectorAll(".set-seg-tab")].find((t) => t.dataset.value === "default");
      ask.dispatchEvent(new window.Event("click"));
    })()`);
    expect(page.run("state.allowDefault"), "the choice is kept").toBe("default");
    expect(page.asked, "and applied to the live session").toContain("colai_allow_now");
  });
});

describe("the first-run hint shows once and not again", () => {
  test("it appears on a cold start, dismisses, and stays gone once seen", async () => {
    const page = openTheToolbar();
    await settle();

    // A toolbar that has never seen it shows it — start() puts it up after placing the rail.
    expect(
      page.dom.document.querySelector(".first-hint"),
      "a fresh toolbar shows the hint",
    ).toBeTruthy();
    // It is a pointer, not a sheet: it offers its own way out.
    expect(
      page.dom.document.querySelector(".first-hint .first-hint-shut"),
      "and the hint carries a dismiss",
    ).toBeTruthy();

    // The harness localStorage is a no-op, so swap in one that actually remembers — the
    // flag's whole job is to survive, and a later run is what reads it back.
    page.run(`(() => {
      const mem = {};
      const store = {
        getItem: (k) => (k in mem ? mem[k] : null),
        setItem: (k, v) => { mem[k] = String(v); },
        removeItem: (k) => { delete mem[k]; },
      };
      window.localStorage = store;
      globalThis.localStorage = store;
    })();`);

    // Clear what the cold start drew, then ask for it fresh against the remembering store.
    page.run(`(document.querySelector(".first-hint") || { remove() {} }).remove(); showFirstHint();`);
    expect(page.dom.document.querySelector(".first-hint"), "an unseen hint shows").toBeTruthy();

    // Dismiss it the way a person would.
    page.run(
      `document.querySelector(".first-hint .first-hint-shut").dispatchEvent(new window.Event("click"));`,
    );
    expect(
      page.dom.document.querySelector(".first-hint"),
      "dismissing takes it off the screen",
    ).toBeNull();
    expect(
      page.run(`window.localStorage.getItem("colai.hint.seen")`),
      "and remembers it was seen",
    ).toBe("1");

    // Asked again — a later render, a later run — it stays gone.
    page.run("showFirstHint();");
    expect(
      page.dom.document.querySelector(".first-hint"),
      "a seen hint never shows again",
    ).toBeNull();
  });
});

describe("what a one-word ask actually sends", () => {
  test("Build with nothing marked does not assert a change nobody described", async () => {
    /*
     * "Build: Make this change." over the word "hey". Build is the mode a fresh toolbar is
     * in and a short word is the commonest first thing anybody types, so this was many
     * people's first message: an instruction telling an agent to make a change that was
     * never described, above the one word they actually wrote.
     */
    const page = openTheToolbar();
    await settle();
    const said = (code: string) => String(page.run(code));
    expect(said('summaryFor([], "build", "hey", null)')).toBe("hey");
    // Two words is an ask, and the mode is said about the words rather than about pictures
    // that are not there.
    expect(said('summaryFor([], "build", "fix the header", null)')).toBe(
      "Build: Do what is asked below.\n\nAsked:\nfix the header",
    );
    // And with something marked, "this change" has something to point at again.
    expect(
      said('summaryFor([{ tool: "box" }], "build", "hey", null)'),
      "the instruction stands when there is a picture",
    ).toContain("Build: Make this change.");
    // The other three modes read correctly on their own and are left alone.
    expect(said('summaryFor([], "ask", "what does the rail do?", null)')).toBe(
      "Ask: Answer the question. Do not change anything yet.\n\nAsked:\nwhat does the rail do?",
    );
  });
});

describe("the letters printed on the keys", () => {
  test("S picks a shape, and again picks the other one", async () => {
    // The rail's shape key has said "Box / circle · S" since it was built, and `s` was in
    // no table at all — so the one letter printed on that key did nothing.
    const page = openTheToolbar();
    await settle();
    const letter = (key: string) => {
      const event = new page.dom.window.Event("keydown") as unknown as Record<string, unknown>;
      event.key = key;
      page.dom.window.dispatchEvent(event);
    };
    expect(page.run("state.tool")).toBe("pointer");
    letter("s");
    expect(page.run("state.tool"), "the first press hands over a box").toBe("box");
    letter("s");
    expect(page.run("state.tool"), "the second the other shape").toBe("circle");
    letter("s");
    expect(page.run("state.tool"), "and back").toBe("box");
    // The letters that already worked still do.
    letter("o");
    expect(page.run("state.tool")).toBe("circle");
    letter("v");
    expect(page.run("state.tool")).toBe("pointer");
  });
});

describe("the receiver's name cannot push Send out of the popup", () => {
  /*
   * A conversation is named after whatever was said in it first, so a receiver called
   * `claude-code-plugin-integration` is one unbreakable word. The popup is a fixed 290px
   * card and its last row holds that name beside Keep and Send now — and a flex item does
   * not shrink below the width of its longest word, whatever its `min-width` says. So the
   * row grew past the card and carried Send through the right-hand edge of it.
   *
   * Said plainly, because it matters for how much this proves: linkedom has no layout
   * engine, so nothing here can measure a box against the box it is in. What is checked is
   * the two halves of the cause — the name is an element that can be given an ellipsis, and
   * the rules that shorten it are in the stylesheet. Whether it now fits is a thing for
   * somebody's eyes.
   */
  const css = readFileSync(new URL("./toolbar/ui/toolbar.css", import.meta.url), "utf8");
  // Anchored to the start of a line, or `.chat-running` finds the rule for
  // `.chat-key[data-working="true"] .chat-running` and reads a different block.
  const ruleFor = (selector: string) => {
    const at = css.indexOf(`\n${selector} {`);
    return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
  };

  test("the name is in an element of its own, so it can be shortened", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.marks.push({ id: "mark-1", tool: "box", note: "", chosen: true,
                                 points: [{ x: 0.2, y: 0.2 }], where: null });
              state.popup = "mark-1";
              state.receiving = { kind: "session", id: "s", name: "claude-code-plugin-integration",
                                  emoji: null, locator: null };
              render()`);
    const to = page.dom.document.querySelector(".popup .popup-to");
    expect(to, "the popup's row names who receives").toBeTruthy();
    const name = to?.querySelector(".popup-to-name");
    expect(name?.textContent, "and it is a span, not the button's own text").toBe(
      "claude-code-plugin-integration",
    );
    // Send is still in the row, which is the thing that was being pushed out of the card.
    expect([...(to?.parentElement?.children ?? [])].length).toBe(3);
  });

  test("and the stylesheet actually shortens it", () => {
    const name = ruleFor(".popup-to-name");
    // `min-width: 0` is the one that is easy to leave out and the one that matters: without
    // it the item stops shrinking at its longest word.
    expect(name, "an unbreakable word needs this to give way at all").toContain("min-width: 0");
    expect(name).toContain("text-overflow: ellipsis");
    expect(name).toContain("white-space: nowrap");
    expect(name).toContain("overflow: hidden");
    // And it gives way completely in the popup, where it is the only thing in the row that
    // can. A shrink factor below one takes only that fraction of the overflow off.
    expect(ruleFor(".popup > .popup-foot > .popup-to")).toContain("flex-shrink: 1");
    // The same treatment on the rail, where the name is drawn beside the light. The
    // picker's rows already had it; these two did not.
    expect(ruleFor(".chat-name")).toContain("min-width: 0");
    expect(ruleFor(".chat-name")).toContain("max-width");
    expect(ruleFor(".chat-who")).toContain("text-overflow: ellipsis");
    expect(ruleFor(".chat-running")).toContain("text-overflow: ellipsis");
  });
});

/*
 * ── Review changes: preview before it happens ────────────────────────────────
 *
 * The opt-in that turns the fast path — edits apply as the agent makes them — into one where
 * every change stops and shows itself first. The platform is for people who do not read code,
 * so the card leads with a picture and folds the diff away; these prove the fold is shut by
 * default, that turning Review on actually makes the session ask, and that a change with no
 * picture to show opens the code on its own rather than showing nothing.
 */
describe("Review changes before they are applied", () => {
  /** A `can_use_tool` for an Edit, the shape the approval card is drawn from. */
  const EDITING = `
    state.asking = {
      id: "ask-1", tool: "Edit",
      input: { file_path: "/home/someone/project/app.css", old_string: "gap: 8px;", new_string: "gap: 12px;" },
      title: "Widen the gap",
    };
    render();
  `;

  test("the toggle lives in Settings, off by default, and flips on click", async () => {
    // Review moved out of the Work composer (it is a standing preference, not a per-send
    // control). It lives in the settings panel now.
    const page = openTheToolbar();
    await settle();
    page.run("openSettings(); render()");
    const toggle = page.dom.document.querySelector('.set-toggle[aria-label="Review changes"]') as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    expect(toggle, "settings carries a Review toggle").toBeTruthy();
    expect(toggle?.getAttribute("aria-pressed"), "off until asked for").toBe("false");
    expect(page.run("state.review")).toBe(false);

    toggle?.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.review"), "a click arms it").toBe(true);
    const armed = page.dom.document.querySelector('.set-toggle[aria-label="Review changes"]');
    expect(armed?.getAttribute("aria-pressed"), "and the toggle shows it").toBe("true");
  });

  test("the Work composer no longer carries the Review / slash / mention buttons", async () => {
    // The person asked for the Work popup to shed these — Review is in Settings, and typing `/`
    // or `@` still opens their menus, so the composer keeps only who / how / send.
    const page = openTheToolbar();
    await settle();
    page.run("state.work.open = true; render()");
    expect(
      page.dom.document.querySelector('[data-field="review"]'),
      "no Review toggle in the composer",
    ).toBeNull();
    expect(
      page.dom.document.querySelector(".compose-key-do"),
      "no / or @ buttons in the composer",
    ).toBeNull();
  });

  test("a remembered Review is restored, and only an explicit true arms it", async () => {
    const on = openTheToolbar({ remembered: '{"x":40,"y":40,"review":true}' });
    await settle();
    on.run("recall()");
    expect(on.run("state.review"), "a saved true comes back on").toBe(true);

    // An install from before Review existed has no opinion, and off is what that must mean.
    const old = openTheToolbar({ remembered: '{"x":40,"y":40}' });
    await settle();
    old.run("recall()");
    expect(old.run("state.review"), "no opinion means off").toBe(false);
  });

  test("turning Review on makes the send ask, before it sends", async () => {
    const page = openTheToolbar({
      answers: {
        colai_send: { sessionKey: "chat-1", prompt: "colai-1", watching: true, pictures: 1, sentTo: true },
      },
    });
    await settle();
    page.run(`
      state.receiving = { kind: "session", id: "chat-1", name: "Claude" };
      state.marks.push({ id: "mark-1", tool: "box", note: "", chosen: true,
                         points: [{ x: 0.2, y: 0.2 }, { x: 0.4, y: 0.4 }], where: null });
      state.review = true;
    `);
    await page.run('sendMarks(["mark-1"])');
    await settle();
    // The mode is set to `default` — every edit will stop — and it is set on the way to the
    // send, not after it.
    expect(page.asked, "Review asks the session to hold edits").toContain("colai_allow_now");
    expect(page.run("state.allowing"), "and the chip reflects it").toBe("default");
    expect(
      page.asked.indexOf("colai_allow_now") <= page.asked.lastIndexOf("colai_send"),
      "the ask goes before the send",
    ).toBe(true);
  });

  test("with Review off the session is left in its fast mode", async () => {
    const page = openTheToolbar({
      answers: {
        colai_send: { sessionKey: "chat-1", prompt: "colai-1", watching: true, pictures: 1, sentTo: true },
      },
    });
    await settle();
    page.run(`
      state.receiving = { kind: "session", id: "chat-1", name: "Claude" };
      state.marks.push({ id: "mark-1", tool: "box", note: "", chosen: true,
                         points: [{ x: 0.2, y: 0.2 }, { x: 0.4, y: 0.4 }], where: null });
      state.review = false;
    `);
    await page.run('sendMarks(["mark-1"])');
    await settle();
    expect(page.asked, "an ordinary send does not touch the permission mode").not.toContain(
      "colai_allow_now",
    );
  });

  test("the preview leads with the picture and folds the code away", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.reviewBefore = ["data:image/png;base64,AAAA"]; ${EDITING}`);
    page.run("drawApproval()");
    const card = page.dom.document.querySelector("#fly-ask");
    expect(card?.querySelector(".review-before-shot"), "the before picture is shown").toBeTruthy();
    const reveal = card?.querySelector(".review-reveal") as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    expect(reveal, "and a way into the code").toBeTruthy();
    expect(reveal?.getAttribute("aria-expanded"), "shut by default").toBe("false");
    expect(card?.querySelector(".ask-diff"), "so no diff is drawn yet").toBeFalsy();

    // The head names the file plainly, not "Edit · …/app.css".
    expect(card?.querySelector(".ask-who")?.textContent).toBe("About to change app.css");

    reveal?.dispatchEvent(new page.dom.window.Event("click"));
    const opened = page.dom.document.querySelector("#fly-ask");
    const rows = [...(opened?.querySelectorAll(".ask-diff-row") ?? [])];
    expect(rows.length, "opening it draws the diff").toBeGreaterThan(0);
    const added = rows.find((r) => (r as HTMLElement).dataset.sign === "+");
    // textContent throughout: the row shows the code as text, never as markup.
    expect(added?.querySelector(".ask-diff-text")?.textContent).toBe("gap: 12px;");
  });

  test("a change with no picture opens the code itself and says why", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.reviewBefore = []; ${EDITING}`);
    page.run("drawApproval()");
    const card = page.dom.document.querySelector("#fly-ask");
    expect(card?.querySelector(".review-note")?.textContent, "it says why").toContain(
      "showing the code change instead",
    );
    expect(card?.querySelector(".ask-diff"), "and the code is there, unfolded").toBeTruthy();
    expect(card?.querySelector(".review-reveal"), "with no fold to open").toBeFalsy();
  });

  test("a Work-panel row has no rewind of its own — going back is per ask, in the thread", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.work.open = true;
      state.work.scope = "all";
      state.history = [
        { at: 2, who: "Claude", sessionKey: "s1", said: "the older change", count: 0,
          prompt: "colai-1", mine: true, view: { open: false, shown: new Set() },
          changes: [{ path: "/p/a.css", kind: "edit", old: "a", new: "b" }],
          answer: { sessionKey: "s1", who: "Claude", turns: [{ said: "Done.", mine: false }] } },
      ];
      render();
    `);
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    expect(row, "the exchange is on screen").toBeTruthy();
    const labels = [...(row?.querySelectorAll(".work-act") ?? [])].map((b) => b.textContent);
    expect(labels, "no row-level rewind").not.toContain("Rewind files to here");
    expect(labels, "and no row-level Copy /rewind").not.toContain("Copy /rewind");
    expect(row?.querySelector(".work-rewind-at"), "folded, there is no thread to rewind from").toBeNull();
  });

  /** A finished, reviewed exchange with a spot on screen — what Compare is offered on. */
  const compareReady = (beforeThumb: string) => `
    state.work.open = true;
    state.work.scope = "all";
    state.history = [{
      at: 3, who: "Claude", sessionKey: "s1", said: "make the header green", count: 0,
      prompt: "colai-1", review: true, mine: true, view: { open: true, shown: new Set() },
      regions: [{ region: { box: { x: 0, y: 0, w: 0.2, h: 0.2 } }, points: [], on: null,
                  thumb: "${beforeThumb}" }],
      answer: { sessionKey: "s1", who: "Claude", turns: [{ said: "Done.", mine: false }] },
    }];
    render();
  `;

  test("Compare photographs the spot again and shows it beside the before", async () => {
    const page = openTheToolbar();
    await settle();
    // Stand in for the real capture: a distinct "after" shot, so this is a change, not sameness.
    page.run(`photograph = async (mark) => { mark.thumb = "data:image/png;base64,AFTER"; };`);
    page.run(compareReady("data:image/png;base64,BEFORE"));

    const row = page.dom.document.querySelector('[data-entry="s1"]');
    const compare = [...(row?.querySelectorAll(".work-act") ?? [])].find(
      (b) => b.textContent === "Compare",
    ) as (Element & { dispatchEvent: (e: unknown) => void }) | undefined;
    expect(compare, "a reviewed change with a spot offers Compare").toBeTruthy();

    compare?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();

    expect(page.run("state.history[0].compare && state.history[0].compare.after")).toBe(
      "data:image/png;base64,AFTER",
    );
    const stage = page.dom.document.querySelector('[data-entry="s1"] .review-compare-stage');
    expect(stage, "the before/after stage is drawn").toBeTruthy();
    expect(
      page.dom.document.querySelector('[data-entry="s1"] .review-compare-slider'),
      "with a slider to wipe between them",
    ).toBeTruthy();
    expect(
      page.dom.document.querySelector('[data-entry="s1"] .review-compare-before')?.getAttribute("src"),
      "the before is the shot kept from the send",
    ).toBe("data:image/png;base64,BEFORE");
  });

  test("an unchanged-looking spot says so rather than faking a difference", async () => {
    const page = openTheToolbar();
    await settle();
    // The new shot comes back identical to the before — the app has not redrawn.
    page.run(`photograph = async (mark) => { mark.thumb = "data:image/png;base64,SAME"; };`);
    page.run(compareReady("data:image/png;base64,SAME"));

    const row = page.dom.document.querySelector('[data-entry="s1"]');
    const compare = [...(row?.querySelectorAll(".work-act") ?? [])].find(
      (b) => b.textContent === "Compare",
    ) as (Element & { dispatchEvent: (e: unknown) => void }) | undefined;
    compare?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();

    expect(page.run("state.history[0].compare && state.history[0].compare.same"), "it is recognised as the same").toBe(
      true,
    );
    const note = page.dom.document.querySelector('[data-entry="s1"] .review-note');
    expect(note?.textContent, "and it says why, honestly").toContain("may not have refreshed");
    // And the action becomes a way to try once more, since the app may refresh in a moment.
    const again = [...(page.dom.document.querySelector('[data-entry="s1"]')?.querySelectorAll(".work-act") ?? [])].find(
      (b) => b.textContent === "Check again",
    );
    expect(again, "with a way to check again").toBeTruthy();
  });

});

/*
 * ── keyboard shortcuts ───────────────────────────────────────────────────────
 *
 * `onKey` is the window handler. These call it directly with a synthetic event, which is
 * how the twelve scripts see a keypress once linkedom has dispatched one — reliable, and it
 * tests the branch order that matters: Escape must close what is open before it stops a run,
 * and only stop when there is nothing left to close.
 */
describe("keyboard shortcuts", () => {
  const RUNNING = `
    state.recording = false; state.popup = null;
    state.receiving = { kind: "session", id: "s1", name: "X" };
    state.history = [{ sessionKey: "s1", at: 2, who: "X", answer: null, mine: true,
                       view: { open: true, shown: new Set() } }];
    state.runs = [{ sessionKey: "s1", who: "X", heard: Date.now() }];
  `;
  const esc = "onKey({ key:'Escape', target:{ tagName:'BODY' }, preventDefault(){} })";

  test("Escape stops a running agent once nothing is left to close", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.work.open = false; ${RUNNING}`);
    const before = page.askedWith.filter(([n]) => n === "colai_stop").length;
    page.run(esc);
    await settle();
    const after = page.askedWith.filter(([n]) => n === "colai_stop").length;
    expect(after, "the run is stopped").toBeGreaterThan(before);
  });

  test("Escape closes an open panel first, and leaves the run alone", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.work.open = true; ${RUNNING}`);
    const before = page.askedWith.filter(([n]) => n === "colai_stop").length;
    page.run(esc);
    await settle();
    expect(page.run("state.work.open"), "the panel closes").toBe(false);
    expect(
      page.askedWith.filter(([n]) => n === "colai_stop").length,
      "nothing is stopped on this press",
    ).toBe(before);
    expect(page.run("state.runs.length"), "the run is still there").toBeGreaterThan(0);
  });

  test("Ctrl+Enter sends the Work composition from anywhere", async () => {
    const page = openTheToolbar({
      answers: {
        colai_send: { sessionKey: "s1", prompt: "colai-1", watching: true, pictures: 1, sentTo: true },
      },
    });
    await settle();
    page.run(`
      state.receiving = { kind: "session", id: "s1", name: "X" };
      state.text = "do the thing";
      state.marks.push({ id: "m1", tool: "pointer", chosen: true, points: [{ x: 0.5, y: 0.5 }], where: null, note: "" });
    `);
    page.run("onKey({ key:'Enter', ctrlKey:true, target:{ tagName:'BODY' }, preventDefault(){} })");
    await settle();
    expect(page.asked, "the composition is sent").toContain("colai_send");
  });

  test("Ctrl+Enter with nothing to send just opens Work", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.work.open = false; state.marks = []; state.files = []; state.text = "";`);
    page.run("onKey({ key:'Enter', metaKey:true, target:{ tagName:'BODY' }, preventDefault(){} })");
    await settle();
    expect(page.run("state.work.open"), "Work opens so the keystroke is not a dead end").toBe(true);
    expect(page.asked, "and nothing was sent").not.toContain("colai_send");
  });
});

/*
 * ── the shared overlay shell ─────────────────────────────────────────────────
 *
 * Additive infrastructure the Context map is built on. It must open a surface, centre it,
 * and be dismissable by Escape and by a press off it — and, crucially, be dormant when no
 * surface is up, so the panels that predate it are untouched.
 */
describe("the shared overlay shell", () => {
  test("openSurface shows and places a surface; Escape and an outside press close it", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.screens = [{ x:0, y:0, width:1920, height:1080, reserved:{ top:0, right:0, bottom:0, left:0 } }];
      state.at = { x: 40, y: 40 };
      const node = document.createElement("div");
      node.className = "overlay-surface";
      node.id = "test-surface";
      node.hidden = true;
      document.body.appendChild(node);
      openSurface("context", node);
    `);
    expect(page.run("state.panel"), "the surface is up").toBe("context");
    expect(page.run(`document.getElementById("test-surface").hidden`), "and shown").toBe(false);
    expect(page.run(`document.getElementById("test-surface").style.position`), "and placed fixed").toBe("fixed");

    page.run(`onKey({ key:"Escape", target:{ tagName:"BODY" }, preventDefault(){} })`);
    expect(page.run("state.panel"), "Escape closes the surface").toBe(null);

    page.run(`openSurface("context", document.getElementById("test-surface"))`);
    expect(page.run("state.panel")).toBe("context");
    page.run(`pressedSomewhereElse({ target: document.body })`);
    expect(page.run("state.panel"), "a press off it closes it").toBe(null);
  });

  test("the shell is dormant when nothing is open", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.panel = null; state.work.open = false; state.popup = null; state.recording = false;`);
    // Escape with no surface must not throw and must not invent a panel — it falls through to
    // the ordinary tool reset.
    expect(() =>
      page.run(`onKey({ key:"Escape", target:{ tagName:"BODY" }, preventDefault(){} })`),
    ).not.toThrow();
    expect(page.run("state.panel")).toBe(null);
  });

  test("the enter is a 180ms animation, and reduced motion drops it", () => {
    const css = readFileSync(new URL("./toolbar/ui/toolbar.css", import.meta.url), "utf8");
    const at = css.indexOf(".overlay-enter {");
    expect(at, "the enter rule exists").toBeGreaterThan(-1);
    expect(css.slice(at, css.indexOf("}", at)), "at the PRD's 180ms").toContain("180ms");
    expect(css, "and suppressed under reduced motion").toMatch(
      /prefers-reduced-motion[\s\S]*?\.overlay-enter\s*\{\s*animation:\s*none/,
    );
  });
});

/*
 * ── the Context map ──────────────────────────────────────────────────────────
 *
 * Local-first: the store is on this machine, Team is empty, and the marquee behaviours are the
 * §8 acceptance — save appears, dropping two contexts on a session sums their tokens (a shared
 * snapshot once), Open resolves and hands the manifest to the session, and the feature flag can
 * take the key off the rail entirely.
 */
describe("the Context map", () => {
  test("a contextMap: true remembered from an older install does not bring the key back", async () => {
    // Before it was hidden, the flag defaulted on and was saved with the rail's position — so
    // every existing install has `contextMap: true` in storage. That must not resurrect it.
    const page = openTheToolbar({ remembered: '{"x":40,"y":40,"contextMap":true}' });
    await settle();
    expect(page.run("state.contextMap"), "the old saved flag is ignored").toBe(false);
    expect(page.run("buttons.context.hidden"), "and the key stays hidden").toBe(true);
  });

  test("the key is dormant behind the flag — off by default, and the flag shows it", async () => {
    const page = openTheToolbar();
    await settle();
    // Ships off: the feature is kept but out of the way, and nothing on the rail reserves
    // space for it. A hidden key is display:none — no reserved space (PRD §9).
    expect(page.run("state.contextMap"), "off by default").toBe(false);
    expect(page.run("buttons.context.hidden"), "and the rail key is hidden").toBe(true);
    // The flag gates visibility in render, so flipping it on shows the key without a rebuild.
    page.run("state.contextMap = true; render()");
    const there = page.run(
      `[...document.querySelectorAll('#rail .key')].some((k) => (k.title||'').startsWith('Context map'))`,
    );
    expect(there, "on the rail once the flag is on").toBe(true);
    expect(page.run("buttons.context.hidden"), "and visible").toBe(false);
  });

  test("opening the map puts it on the shell and asks for the local set", async () => {
    const page = openTheToolbar({ answers: { colai_contexts_list: [] } });
    await settle();
    page.run("openContextMap()");
    await settle();
    expect(page.run("state.panel"), "up on the shell").toBe("context");
    expect(page.asked, "loaded the local contexts").toContain("colai_contexts_list");
    expect(page.dom.document.querySelector("#context .ctx-head"), "the panel is drawn").toBeTruthy();
  });

  test("Team is the empty state here — nobody has shared one", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.contextScope = "team"; state.contexts = []; openSurface("context", el.context); render();`);
    expect(page.dom.document.querySelector("#context .ctx-empty")?.textContent).toContain(
      "Nobody on your team",
    );
  });

  test("Save this session saves it as a context", async () => {
    const page = openTheToolbar({
      answers: {
        colai_context_save_session: { id: "ctx-1", title: "My session", owner: "you", machine: "pc", tokens: 10, payloadRef: "p", isLocal: true, visibility: "private", updatedAt: 1, project: "" },
        colai_contexts_list: [],
      },
    });
    await settle();
    page.run(`state.receiving = { id: "s1", name: "My session" }; saveThisSession();`);
    await settle();
    expect(
      page.askedWith.find(([n]) => n === "colai_context_save_session")?.[1],
      "saved from the received session",
    ).toMatchObject({ sessionKey: "s1" });
  });

  test("dropping two contexts on a session sums their tokens; Open resolves and sends", async () => {
    const page = openTheToolbar({
      answers: {
        colai_context_resolve: { contexts: [], tokens: 300, window: 180000, overflow: false, payloads: 2 },
        colai_send: { sessionKey: "s1" },
      },
    });
    await settle();
    page.run(`
      state.contexts = [
        { id:"c1", title:"A", owner:"you", machine:"pc", tokens:100, payloadRef:"p1", isLocal:true },
        { id:"c2", title:"B", owner:"you", machine:"pc", tokens:200, payloadRef:"p2", isLocal:true },
      ];
      state.sessions = [{ key:"s1", title:"Session one" }];
      state.receiving = { id:"s1", name:"Session one" };
      openSurface("context", el.context);
      stageContext("s1","c1");
      stageContext("s1","c2");
      render();
    `);
    expect(page.run(`stagedTokens("s1")`), "the pill sums both").toBe(300);
    expect(page.dom.document.querySelector("#context .ctx-pill")?.textContent).toContain("300");

    page.run(`openStaged("s1")`);
    await settle();
    expect(
      page.askedWith.find(([n]) => n === "colai_context_resolve")?.[1],
      "Open resolves the staged ids",
    ).toEqual({ ids: ["c1", "c2"] });
    expect(page.asked, "and hands the manifest to the session").toContain("colai_send");
  });

  test("a shared snapshot is counted once in the pill", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.contexts = [
        { id:"c1", title:"A", tokens:100, payloadRef:"same", isLocal:true, owner:"you", machine:"pc" },
        { id:"c2", title:"B", tokens:100, payloadRef:"same", isLocal:true, owner:"you", machine:"pc" },
      ];
      state.staged = { s1: ["c1","c2"] };
    `);
    expect(page.run(`stagedTokens("s1")`), "one snapshot, one count").toBe(100);
  });
});

/*
 * ── the rail's six groups ────────────────────────────────────────────────────
 *
 * §3 lays the rail out in six groups, each after its own divider. This walks the rail in DOM
 * order and checks the keys fall into the right groups — the layout the regroup was for.
 */
describe("the rail groups", () => {
  test("the keys are laid out in the six PRD groups, in order", async () => {
    const page = openTheToolbar();
    await settle();
    const raw = page.run(`(() => {
      const groups = [[]];
      for (const child of el.rail.children) {
        if (child.hasAttribute && child.hasAttribute("data-divider")) { groups.push([]); continue; }
        if (child.classList && child.classList.contains("grip")) continue;
        if (child.classList && child.classList.contains("key")) groups[groups.length - 1].push(child.title || "");
      }
      return JSON.stringify(groups);
    })()`) as string;
    const groups = (JSON.parse(raw) as string[][]).filter((g) => g.length > 0);
    expect(groups.length, "six groups").toBe(6);
    expect(groups[0], "select").toEqual(["Pointer · V", "Point at · P"]);
    expect(groups[1][0], "mark-up leads with Draw").toMatch(/^Draw/);
    expect(groups[1], "mark-up holds Box / circle").toContain("Box / circle · S");
    expect(groups[2], "make: Design, Git, Context map").toEqual(["Design", "Git", "Context map · K"]);
    expect(groups[3], "utilities holds the exact tools").toEqual(
      expect.arrayContaining(["Measure · M", "Colour · C", "Screenshot"]),
    );
    expect(groups[3].length, "utilities is the fold key plus the exact tools").toBe(5);
    expect(groups[4], "history").toEqual(["Undo · Ctrl Z", "Redo · Ctrl Shift Z"]);
    expect(groups[5][0], "work leads with the clock, to the left of Work").toMatch(/^Scheduled tasks/);
    expect(groups[5].some((t) => t.startsWith("Work")), "the Work key is in the group").toBe(true);
    expect(groups[5], "work holds the conversation key").toContain("Who receives this?");
    expect(groups[5].length, "clock, send, conversation, stop, the critter and close").toBe(6);
    expect(groups[5][5], "close is the last key on the rail").toBe("Close Colai");
  });
});

describe("the jellyfish walks whenever work is underway", () => {
  test("a mark you sent makes it jiggle, even with an empty Gateway snapshot", async () => {
    const page = openTheToolbar();
    await settle();
    // No Gateway-wide work, but a send of your own is in flight (a handed/mirrored turn is only
    // ever visible here, never in atWork).
    page.run(`
      state.atWork = null;
      state.runs = [{ sessionKey: "s1", who: "Claude", heard: Date.now() }];
      render();
    `);
    expect(page.run("buttons.settings.dataset.walking"), "the mascot walks").toBe("true");
    expect(page.run("buttons.settings.dataset.mood")).toBe("working");
  });

  test("it holds still when nothing is running", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.atWork = null; state.runs = []; render();`);
    expect(page.run("buttons.settings.dataset.walking")).toBe("false");
    expect(page.run("'mood' in buttons.settings.dataset"), "no mood attribute at rest").toBe(false);
  });

  test("Gateway trouble still wins over a local run", async () => {
    const page = openTheToolbar();
    await settle();
    // Something elsewhere is in trouble while a local send works: the mascot must not paper over
    // the trouble with a walk, so trouble wins and it does not walk.
    page.run(`
      state.atWork = { running: 1, waiting: 0, trouble: 1 };
      state.runs = [{ sessionKey: "s1", who: "Claude", heard: Date.now() }];
      render();
    `);
    expect(page.run("buttons.settings.dataset.mood")).toBe("trouble");
    expect(page.run("buttons.settings.dataset.walking"), "no walk under a trouble light").toBe("false");
  });
});

/*
 * ── Box / Record capture menus ───────────────────────────────────────────────
 *
 * The "soon" stubs the PRD named but whose backends weren't built (whole-window mapping, AT-SPI
 * snap, an input overlay, a rolling buffer) have been dropped — a menu is a true account of what
 * the tool does, and a row that only says "not yet" is not worth the space. What remains is the
 * built set: Box and Circle to shape a region, and the length rows for a region recording.
 */
describe("the Box and Record capture menus", () => {
  test("the Box menu offers the built shapes, and none of the dropped stubs", async () => {
    const page = openTheToolbar();
    await settle();
    const labels = page.run(
      `[...document.querySelectorAll('#fly-shape .row')].map((r) => r.textContent)`,
    ) as string[];
    // Each built shape carries its shortcut badge, so the label reads "BoxB" / "CircleO".
    expect(labels.some((l) => l.startsWith("Box")), "Box is a built shape").toBe(true);
    expect(labels.some((l) => l.startsWith("Circle")), "Circle is a built shape").toBe(true);
    expect(labels, "the stubs are gone").not.toEqual(
      expect.arrayContaining(["Whole window", "Snap to element"]),
    );
  });

  test("the Record menu offers the length rows, and none of the dropped stubs", async () => {
    const page = openTheToolbar();
    await settle();
    const labels = page.run(
      `[...document.querySelectorAll('#fly-record .row')].map((r) => r.textContent)`,
    ) as string[];
    // The length rows (a region recording — the built default) are still there.
    expect(labels.some((l) => /seconds$/.test(l) && /^\d/.test(l)), "the length rows remain").toBe(true);
    expect(labels, "the stubs are gone").not.toEqual(
      expect.arrayContaining(["Whole window", "Include clicks & keys", "Last 30 seconds"]),
    );
  });

  test("no soon stubs are left in the source or the stylesheet", () => {
    const rail = readFileSync(new URL("./toolbar/ui/toolbar-rail.js", import.meta.url), "utf8");
    expect(rail, "the soonRow helper is gone").not.toContain("function soonRow");
    expect(rail, "and nothing marks a soon row").not.toContain('dataset.soon');
    const css = readFileSync(new URL("./toolbar/ui/toolbar.css", import.meta.url), "utf8");
    expect(css, "and the stylesheet no longer styles one").not.toContain('data-soon');
  });
});

/*
 * ── the response card beside the toolbar (Agent Responses) ─────────────────
 *
 * A watching send opens a card by the rail (not out at the mark); it shows the turn's kind/state
 * from state.turns, streams/settles the answer, carries a reply and follow-ups, and Esc dismisses
 * it without touching the turn or its Work-log row.
 */
describe("the response card beside the toolbar", () => {
  const WATCHING = {
    colai_send: { sessionKey: "s1", prompt: "colai-1", watching: true, pictures: 1, sentTo: true },
  };
  const sendOne = `
    state.receiving = { kind: "session", id: "s1", name: "Claude" };
    state.text = "check this";
    state.marks.push({ id: "m1", tool: "box", chosen: true, points: [{ x: 0.3, y: 0.3 }, { x: 0.5, y: 0.5 }], region: { box: { x: 0.3, y: 0.3, w: 0.2, h: 0.2 } }, where: null, note: "" });
  `;

  test("a watching send opens the card beside the toolbar, with the prompt quoted", async () => {
    const page = openTheToolbar({ answers: WATCHING });
    await settle();
    page.run(sendOne);
    await page.run('sendMarks(["m1"])');
    await settle();
    expect(page.run("state.responding && state.responding.turnId"), "the card is bound to the turn").toBe("s1");
    const card = page.dom.document.querySelector("#response");
    expect(card?.hidden, "and it is shown").toBe(false);
    expect(card?.querySelector(".resp-quote")?.textContent, "the prompt is quoted").toContain("check this");
  });

  test("a new send clears the previous turn's answer, so the card never flashes stale prose", async () => {
    const page = openTheToolbar({ answers: WATCHING });
    await settle();
    page.run(sendOne);
    await page.run('sendMarks(["m1"])');
    await settle();
    // The first turn settles to a done answer with body/steps (a handed turn reuses "s1" as turnId).
    page.run(`state.turns["s1"] = { kind: "answer", state: "done", body: "OLD ANSWER",
      steps: [{ id: "e", said: "Editing x", verb: "edit", done: true }] }; render();`);
    expect(page.run(`state.turns["s1"].body`), "set up: the old answer is present").toBe("OLD ANSWER");
    // A second send to the same receiver reuses turnId "s1".
    page.run(`state.text = "second ask"; state.marks.push({ id: "m2", tool: "box", chosen: true, points: [{ x: 0.3, y: 0.3 }, { x: 0.5, y: 0.5 }], region: { box: { x: 0.3, y: 0.3, w: 0.2, h: 0.2 } }, where: null, note: "" });`);
    await page.run('sendMarks(["m2"])');
    await settle();
    expect(page.run(`state.turns["s1"].state`), "the new turn starts working, not the old done").toBe("working");
    expect(page.run(`state.turns["s1"].body`), "and carries none of the old prose").toBeUndefined();
    expect(page.run(`(state.turns["s1"].steps || []).length`), "nor the old steps").toBe(0);
    // The card shows the working checklist, not the settled answer.
    expect(page.dom.document.querySelector("#response .resp-answer"), "no stale answer block").toBeNull();
  });

  test("a non-watching send opens no card (answered on their own screen)", async () => {
    const page = openTheToolbar({ answers: { colai_send: { sessionKey: "s1", prompt: "c1", watching: false, handedTo: "Term", pictures: 1 } } });
    await settle();
    page.run(sendOne);
    await page.run('sendMarks(["m1"])');
    await settle();
    expect(page.run("state.responding"), "nothing to show here").toBe(null);
    expect(page.dom.document.querySelector("#response")?.hidden).toBe(true);
  });

  test("the answer body and follow-ups render once the turn settles", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "check this", marks: [] };
      state.turns = { s1: { kind: "answer", state: "done", body: "Here is what you asked.", followUps: ["Open a fix", "Show the query"] } };
      render();
    `);
    const card = page.dom.document.querySelector("#response");
    expect(card?.dataset.state).toBe("done");
    expect(card?.querySelector(".md .md-p")?.textContent, "the reply renders as markdown").toBe("Here is what you asked.");
    expect(card?.querySelectorAll(".resp-follow").length, "up to three follow-up chips").toBe(2);
  });

  test("a working turn shows the plain-language checklist, not the streaming prose", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "x", marks: [] };
      state.turns = { s1: { kind: "answer", state: "working", body: "I'll take a look first…", steps: [
        { id: "a", said: "Reading a file", verb: "read", done: true },
        { id: "b", said: "Writing snake.html", verb: "create", file: "snake.html", done: false },
      ] } };
      render();
    `);
    const card = page.dom.document.querySelector("#response");
    expect(card?.dataset.state).toBe("working");
    // The checklist, not the prose the agent was streaming.
    expect(card?.querySelector(".resp-answer"), "no prose while working").toBeNull();
    const steps = card?.querySelectorAll(".resp-step");
    expect(steps?.length).toBe(2);
    expect(steps?.[0].getAttribute("data-state"), "the finished step is ticked").toBe("done");
    expect(steps?.[1].getAttribute("data-state"), "the current step is now").toBe("now");
    expect(steps?.[1].querySelector(".resp-step-said")?.textContent).toBe("Writing snake.html");
  });

  test("a settled turn leads with what changed, from the steps not the prose", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "write a snake game", marks: [] };
      state.turns = { s1: { kind: "answer", state: "done", body: "All done — have fun.", steps: [
        { id: "b", said: "Writing snake.html", verb: "create", file: "snake.html", done: true },
      ] } };
      render();
    `);
    const card = page.dom.document.querySelector("#response");
    expect(card?.querySelector(".resp-outcome-said")?.textContent, "the outcome headline").toContain("Created snake.html");
  });

  test("a long reply is folded to a few lines, and Show more reveals it", async () => {
    const page = openTheToolbar();
    await settle();
    const long = "x".repeat(400);
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "x", marks: [] };
      state.turns = { s1: { kind: "answer", state: "done", body: ${JSON.stringify(long)} } };
      render();
    `);
    let card = page.dom.document.querySelector("#response");
    expect(card?.querySelector(".md.md-fold"), "folded by default").toBeTruthy();
    const more = card?.querySelector(".resp-more") as HTMLButtonElement | null;
    expect(more?.textContent).toBe("Show more");
    more?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    card = page.dom.document.querySelector("#response");
    expect(card?.querySelector(".md.md-fold"), "revealed — the fold is gone").toBeNull();
    expect((card?.querySelector(".resp-more") as HTMLElement)?.textContent).toBe("Show less");
  });

  test("colai:doing and colai:did build the turn's checklist", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.turns = { s1: { kind: "answer", state: "working" } };
      noteStep({ sessionKey: "s1", id: "c1", name: "Write", args: { file_path: "/p/snake.html" } });
      finishStep({ sessionKey: "s1", id: "c1" });
      noteStep({ sessionKey: "s1", id: "c1", name: "Write", args: { file_path: "/p/snake.html" } });
    `);
    expect(page.run("state.turns.s1.steps.length"), "the repeat call is deduped by id").toBe(1);
    expect(page.run("state.turns.s1.steps[0].done")).toBe(true);
    expect(page.run("state.turns.s1.steps[0].verb")).toBe("create");
    expect(page.run("state.turns.s1.steps[0].file")).toBe("snake.html");
  });

  test("an Edit records a reversible before/after; a Write does not", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.turns = { s1: { kind: "answer", state: "working" } };
      noteStep({ sessionKey: "s1", id: "e1", name: "Edit", args: { file_path: "/p/snake.html", old_string: "color: #fff", new_string: "color: #f87171" } });
      noteStep({ sessionKey: "s1", id: "w1", name: "Write", args: { file_path: "/p/new.html", content: "<html>" } });
    `);
    expect(page.run("state.turns.s1.steps[0].change.kind")).toBe("edit");
    expect(page.run("state.turns.s1.steps[0].change.old")).toBe("color: #fff");
    expect(page.run("state.turns.s1.steps[0].change.new")).toBe("color: #f87171");
    expect(page.run("state.turns.s1.steps[1].change.kind"), "a write is marked unrevertible").toBe("write");
  });

  test("a web edit shows a rendered screenshot (not code) with Keep / Undo; Undo reverts on disk", async () => {
    const shot = "data:image/png;base64,RENDERED";
    const page = openTheToolbar({ answers: { colai_render_shot: shot, colai_revert_edits: { restored: [{ file: "snake.html", path: "/p/snake.html" }], skipped: [] } } });
    await settle();
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "make it blue", marks: [], thumb: "data:image/png;base64,AAAA" };
      state.turns = { s1: { kind: "answer", state: "done", body: "Done.", steps: [
        { id: "e1", said: "Editing snake.html", file: "snake.html", verb: "edit", done: true,
          change: { path: "/p/snake.html", kind: "edit", old: "background: #a855f7", new: "background: #3b82f6" } },
      ] } };
      render();
    `);
    await settle(); // let the auto-render resolve

    // It renders the edited web page and shows that screenshot — never the raw code, never a before.
    const rendered = page.askedWith.find(([n]) => n === "colai_render_shot")?.[1] as { file: string } | undefined;
    expect(rendered?.file, "it renders the edited page").toBe("/p/snake.html");
    const card = page.dom.document.querySelector("#response");
    expect((card?.querySelector(".resp-after-shot img") as HTMLImageElement | null)?.src, "the card shows the render").toBe(shot);
    expect(card?.querySelector(".resp-after-val"), "no code shown as the after").toBeNull();
    expect(card?.querySelector(".resp-before"), "never a 'before'").toBeNull();
    expect(card?.textContent, "no raw CSS leaking into the card").not.toContain("#3b82f6");
    expect(card?.querySelector(".resp-keep"), "Keep").toBeTruthy();

    // See the code (inside the action row) still reveals the exact change for anyone who wants it.
    (card?.querySelector(".resp-review-row .resp-more") as (Element & { dispatchEvent: (e: unknown) => void }))
      ?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    const card2 = page.dom.document.querySelector("#response");
    expect(card2?.querySelector(".resp-diff-add")?.textContent).toBe("background: #3b82f6");

    // Undo reverses it on disk via the revert command.
    (card2?.querySelector(".resp-undo") as (Element & { dispatchEvent: (e: unknown) => void }))
      ?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    const sent = page.askedWith.find(([n]) => n === "colai_revert_edits")?.[1] as
      | { edits: { path: string; kind: string }[] }
      | undefined;
    expect(sent?.edits?.[0]?.path).toBe("/p/snake.html");
    expect(page.run("state.responding"), "a clean undo dismisses the card").toBe(null);
  });

  test("a review screenshot enlarges on click and closes on Escape", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`openZoom("data:image/png;base64,BIG")`);
    const z = page.dom.document.querySelector("#zoom");
    expect(z?.hidden, "the lightbox is shown").toBe(false);
    expect((z?.querySelector("img") as HTMLImageElement | null)?.src).toBe("data:image/png;base64,BIG");
    page.run("onKey({ key:'Escape', target:{ tagName:'BODY' }, preventDefault(){} })");
    expect(page.run("state.zoom"), "Escape closes it").toBe(null);
    expect(page.dom.document.querySelector("#zoom")?.hidden, "and hides the lightbox").toBe(true);
  });

  test("Escape dismisses the card, leaving the turn and its record", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.responding = { turnId: "s1", at: { x: 0.4, y: 0.4 }, who: "Claude", said: "x", marks: [] };
      state.turns = { s1: { kind: "answer", state: "done", body: "Kept." } };
      render();
    `);
    page.run("onKey({ key:'Escape', target:{ tagName:'BODY' }, preventDefault(){} })");
    expect(page.run("state.responding"), "the card is gone").toBe(null);
    expect(page.run(`state.turns.s1 && state.turns.s1.body`), "but the turn is kept").toBe("Kept.");
    expect(page.dom.document.querySelector("#response")?.hidden).toBe(true);
  });
});

describe("agent text renders as markdown", () => {
  function mount(page: ReturnType<typeof openTheToolbar>, md: string) {
    page.run(`
      (() => {
        const d = document.createElement("div");
        d.id = "mdtest";
        d.className = "md";
        d.append(renderMarkdown(${JSON.stringify(md)}));
        document.body.appendChild(d);
      })()
    `);
    return page.dom.document.querySelector("#mdtest");
  }

  test("headings, bold, inline code and lists become real elements", async () => {
    const page = openTheToolbar();
    await settle();
    const md = mount(page, "# Title\n\nSome **bold** and `code` here.\n\n- one\n- two");
    expect(md?.querySelector(".md-h1")?.textContent, "a heading").toBe("Title");
    expect(md?.querySelector("strong")?.textContent, "bold").toBe("bold");
    expect(md?.querySelector(".md-ic")?.textContent, "inline code").toBe("code");
    expect(md?.querySelectorAll(".md-list li").length, "two list items").toBe(2);
  });

  test("a fenced code block is a mono panel with the body as text and a Copy button", async () => {
    const page = openTheToolbar();
    await settle();
    const md = mount(page, "Here:\n\n```js\nconst x = 1 < 2;\n```\n");
    expect(md?.querySelector(".md-code code")?.textContent, "the code body is kept verbatim as text").toBe("const x = 1 < 2;");
    expect(md?.querySelector(".md-codelang")?.textContent).toBe("js");
    expect((md?.querySelector(".md-copy") as HTMLElement | null)?.textContent).toBe("Copy");
  });

  test("markup in a reply is inert — rendered as text, never as elements", async () => {
    const page = openTheToolbar();
    await settle();
    const md = mount(page, "watch out <script>alert(1)</script> and <b>x</b>");
    expect(md?.querySelector("script"), "no script element is created").toBeNull();
    expect(md?.querySelector("b"), "no injected element").toBeNull();
    expect(md?.textContent, "the markup shows as literal text").toContain("<script>alert(1)</script>");
  });
});

describe("the Work panel renders agent turns as markdown", () => {
  test("an agent turn renders markdown; the person's own reply stays plain", async () => {
    const page = openTheToolbar();
    await settle();
    const got = page.run(`
      (() => {
        const entry = { view: { open: true, shown: new Set() } };
        const agent = turnSaid(entry, { said: "Do **this**:\\n\\n- step one\\n- step two", mine: false }, 0);
        const mine = turnSaid(entry, { said: "no, do **that**", mine: true }, 1);
        return {
          agentMd: agent.className.indexOf("md") >= 0,
          agentBold: !!agent.querySelector("strong"),
          agentItems: agent.querySelectorAll(".md-list li").length,
          minePlain: mine.tagName.toLowerCase() === "p" && mine.dataset.mine === "true",
          mineNoBold: !mine.querySelector("strong"),
          mineText: mine.textContent,
        };
      })()
    `) as Record<string, unknown>;
    expect(got.agentMd, "agent turn uses the md container").toBe(true);
    expect(got.agentBold, "agent markdown is rendered (bold)").toBe(true);
    expect(got.agentItems, "its list becomes list items").toBe(2);
    expect(got.minePlain, "the person's reply stays a plain quoted paragraph").toBe(true);
    expect(got.mineNoBold, "the person's reply is not markdown-parsed").toBe(true);
    expect(got.mineText, "the person's literal text, markers and all").toBe("no, do **that**");
  });
});

describe("the settings panel — tabs, close, and Claude Code", () => {
  const CLAUDE = {
    colai_claude_settings: {
      plugins: [
        { name: "colai@colai", scope: "local", enabled: true },
        { name: "x@m", scope: "user", enabled: false },
      ],
      skills: [{ name: "refactor", scope: "project" }],
      mcp: [{ name: "github", transport: "stdio", scope: "user", enabled: true }],
      settings: {},
    },
    colai_claude_toggle: null,
  };

  test("the header carries a close button that dismisses the panel", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSettings()");
    expect(page.run("state.panel")).toBe("settings");
    const shut = page.dom.document.querySelector("#settings .set-shut") as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    expect(shut, "a close button").toBeTruthy();
    shut?.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.panel"), "closed").toBe(null);
  });

  test("it opens on the colai tab, with the toggles and how-to", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSettings()");
    expect(page.run("state.settingsTab"), "colai by default").toBe("colai");
    expect(page.dom.document.querySelector('#settings .set-toggle[aria-label="Review changes"]'), "Review is here").toBeTruthy();
    expect(page.dom.document.querySelector("#settings")?.textContent, "the how-to is here").toContain("Drag the grip");
  });

  test("the Claude Code tab lists plugins / skills / MCP and toggles a plugin", async () => {
    const page = openTheToolbar({ answers: CLAUDE });
    await settle();
    page.run("openSettings()");
    const claudeTab = [...page.dom.document.querySelectorAll("#settings .set-tab")].find(
      (t) => t.textContent === "Claude Code",
    ) as (Element & { dispatchEvent: (e: unknown) => void }) | undefined;
    claudeTab?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    expect(page.run("state.settingsTab")).toBe("claude");
    const text = page.dom.document.querySelector("#settings")?.textContent || "";
    expect(text, "a skill is listed").toContain("refactor");
    expect(text, "an MCP server is listed").toContain("github");
    expect(text, "a plugin is listed").toContain("colai@colai");

    const toggle = page.dom.document.querySelector('#settings .set-toggle[aria-label="x@m"]') as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    expect(toggle?.getAttribute("aria-pressed"), "x@m is off").toBe("false");
    toggle?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    const sent = page.askedWith.find(([n]) => n === "colai_claude_toggle")?.[1] as
      | { name: string; enable: boolean }
      | undefined;
    expect(sent?.name, "the right plugin").toBe("x@m");
    expect(sent?.enable, "toggled on").toBe(true);
  });
});

describe("scheduled tasks — the clock key, the composer, and the list", () => {
  test("a clock key sits on the rail, to the left of Work", async () => {
    const page = openTheToolbar();
    await settle();
    const keys = [...page.dom.document.querySelectorAll("#rail .key")];
    const clock = keys.findIndex((k) => k.getAttribute("title") === "Scheduled tasks");
    const work = keys.findIndex((k) => k.classList.contains("send-key"));
    expect(clock, "the clock key is present").toBeGreaterThanOrEqual(0);
    expect(work, "the Work key is present").toBeGreaterThanOrEqual(0);
    expect(clock, "the clock is to the left of Work").toBeLessThan(work);
  });

  test("the clock key opens the panel, and its × closes it", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSchedule()");
    expect(page.run("state.panel")).toBe("schedule");
    const shut = page.dom.document.querySelector("#schedule .sched-shut") as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    expect(shut, "a close button").toBeTruthy();
    shut?.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.panel"), "closed").toBe(null);
  });

  test("it lists the tasks read back from disk", async () => {
    const page = openTheToolbar({
      answers: {
        colai_schedule_list: [
          { id: "sch-1", prompt: "summarise the inbox", images: [], cadence: { kind: "daily", label: "Daily at 9:00 AM" }, enabled: true, createdAt: 1, nextRun: 9e14 },
        ],
      },
    });
    await settle();
    page.run("openSchedule()");
    await settle();
    const text = page.dom.document.querySelector("#schedule")?.textContent || "";
    expect(text, "the task's prompt is shown").toContain("summarise the inbox");
    expect(text, "its cadence is shown").toContain("Daily at 9:00 AM");
  });

  test("Schedule it sends the prompt, a computed next-run and the cadence", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSchedule()");
    await settle();
    // Type into the composer the way a person would, and choose a daily cadence.
    page.run(`state.scheduleDraft.prompt = "water the plants"; state.scheduleDraft.kind = "daily"; state.scheduleDraft.time = "09:00"; render()`);
    const add = page.dom.document.querySelector("#schedule .sched-add") as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    add?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    const sent = page.askedWith.find(([n]) => n === "colai_schedule_add")?.[1] as
      | { prompt: string; nextRun: number; cadence: { kind: string } }
      | undefined;
    expect(sent?.prompt, "the prompt travels").toBe("water the plants");
    expect(sent?.cadence?.kind, "the cadence kind travels").toBe("daily");
    expect(typeof sent?.nextRun, "a next-run epoch is computed here").toBe("number");
    expect(sent!.nextRun, "it is in the future").toBeGreaterThan(Date.now() - 1000);
  });

  test("a task scheduled while pointing at something carries the marks' ids", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("openSchedule()");
    await settle();
    page.run(`state.marks = [{ id: "mark-a", tool: "box" }, { id: "mark-b", tool: "box" }]; state.scheduleDraft.prompt = "check this each morning"; render()`);
    const add = page.dom.document.querySelector("#schedule .sched-add") as
      | (Element & { dispatchEvent: (e: unknown) => void })
      | null;
    add?.dispatchEvent(new page.dom.window.Event("click"));
    await settle();
    const sent = page.askedWith.find(([n]) => n === "colai_schedule_add")?.[1] as
      | { markIds: string[] }
      | undefined;
    expect(sent?.markIds, "the pointed-at marks are attached").toEqual(["mark-a", "mark-b"]);
  });
});

describe("flyouts stay on the screen wherever the toolbar is", () => {
  test("a menu whose dock side would run it off the screen flips to the side with room", async () => {
    const page = openTheToolbar();
    await settle();
    const node = page.dom.document.getElementById("fly-chat") as unknown as {
      hidden: boolean;
      style: Record<string, string>;
      getBoundingClientRect: unknown;
    };
    node.hidden = false;
    // Simulate the real geometry the flat harness otherwise hides: opening upward (dock "bottom")
    // puts the menu's top above the screen; opening downward fits. Measured off which side is set.
    Object.defineProperty(node, "getBoundingClientRect", {
      configurable: true,
      value: () => {
        const up = node.style.bottom && node.style.bottom !== "";
        const box = up
          ? { top: -60, bottom: 40, left: 10, right: 110, width: 100, height: 100, x: 10, y: -60 }
          : { top: 20, bottom: 120, left: 10, right: 110, width: 100, height: 100, x: 10, y: 20 };
        return { ...box, toJSON: () => box };
      },
    });
    // A bottom-docked horizontal rail opens its menus upward by default.
    page.run('state.dock = "bottom"');
    page.run('placeFlyout(document.getElementById("fly-chat"), false, 0)');
    expect(node.style.top, "flipped to open downward").toContain("calc(100%");
    expect(node.style.bottom, "the upward (off-screen) side is cleared").toBe("");
  });
});


/*
 * ── the Work panel as a conversation, and the way back ────────────────────────
 *
 * A row reads the way a conversation does: what you asked in your own words, then what came
 * back. Opened, it is a thread under You / Claude. And "Rewind files to here" puts back the
 * edits colai watched a conversation make — previewed first, then done, then said.
 */
describe("the Work panel reads as a conversation", () => {
  type Clickable = Element & { dispatchEvent: (e: unknown) => void };
  const click = (page: ReturnType<typeof openTheToolbar>, node: Element | null | undefined) =>
    (node as Clickable | null | undefined)?.dispatchEvent(new page.dom.window.Event("click"));
  const button = (row: Element | null | undefined, label: string) =>
    [...(row?.querySelectorAll(".work-act") ?? [])].find((b) => b.textContent === label);

  test("a folded row leads with the prompt, then a preview of the reply, with the folder as a tag", async () => {
    const page = openTheToolbar({
      answers: {
        colai_sessions: [
          { key: "s1", title: "Header colours", preview: "colai-clawhub", lastPrompt: "make the header green", at: 5 },
        ],
      },
    });
    await settle();
    page.run(`
      state.work.open = true;
      state.work.scope = "all";
      state.history[0].answer = { sessionKey: "s1", who: "Header colours",
        turns: [{ said: "Done — the header is **green** now.", mine: false }] };
      render();
    `);
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    expect(row?.querySelector(".work-ask-said")?.textContent, "your words lead, not the folder").toBe(
      "make the header green",
    );
    expect(row?.querySelector(".work-folder")?.textContent, "the folder is a tag").toBe("colai-clawhub");
    expect(row?.querySelector(".work-preview")?.textContent, "the reply, as a glance").toBe(
      "Done — the header is green now.",
    );
    expect(row?.querySelector(".work-thread"), "folded, there is no thread").toBeNull();
    expect(row?.querySelector(".resp-outcome-said")?.textContent, "and it says it was answered").toContain(
      "Answered",
    );
  });

  test("opened, the thread has You and Claude, one Claude turn per answer, and only your typed words", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.work.open = true;
      state.work.scope = "all";
      const composed = summaryFor([{ tool: "box" }, { tool: "box" }], "build", "fix the gap\\n\\nand the colour", null);
      state.history = [{
        at: 3, who: "Claude", sessionKey: "s1", said: "a later ask", count: 0, mine: true,
        view: { open: true, shown: new Set() },
        answer: { sessionKey: "s1", who: "Claude", turns: [
          { said: composed, mine: true },
          { said: "Looking at the gap first.", mine: false },
          { said: "Fixed both.", mine: false },
        ] },
      }];
      render();
    `);
    const thread = page.dom.document.querySelector('[data-entry="s1"] .work-thread');
    expect(thread, "an open row draws its thread").toBeTruthy();
    const speakers = [...(thread?.querySelectorAll(".work-speaker > span:first-child") ?? [])].map(
      (s) => s.textContent,
    );
    expect(speakers, "each turn is labelled").toEqual(["You", "Claude"]);
    expect(thread?.querySelectorAll(".work-said.md").length, "two fragments are one answer").toBe(1);
    const mine = thread?.querySelector('p[data-mine="true"]');
    expect(mine?.textContent, "only what was typed, not the composed message").toBe(
      "fix the gap\n\nand the colour",
    );
    expect(thread?.querySelector(".work-chip")?.textContent, "the marks are counted").toBe("2 marks");
  });

  test("typed words come out of a composed message, an older one, and a plain one", async () => {
    const page = openTheToolbar();
    await settle();
    const part = (code: string) => JSON.parse(String(page.run(`JSON.stringify(typedPart(${code}))`)));
    // After the marker, whole — two paragraphs stay two paragraphs.
    expect(part('summaryFor([{ tool: "box" }], "ask", "one\\n\\ntwo", null)')).toEqual({ said: "one\n\ntwo", marks: 1 });
    // A message from before the marker: its last paragraph, which is where the words went.
    expect(part('"Not inside any window\\n\\n1. Box (mark-1.png)\\n\\nAsk: Answer the question. Do not change anything yet.\\n\\nwhy is it red"')).toEqual(
      { said: "why is it red", marks: 1 },
    );
    // Marks and no words is no words, not colai's own instruction.
    expect(part('summaryFor([{ tool: "box" }], "ask", "", null)').said).toBe("");
    // Somebody typing in a terminal is theirs, whole.
    expect(part('"first\\n\\nsecond"')).toEqual({ said: "first\n\nsecond", marks: 0 });
  });

  test("the prompt the row leads with is not drawn twice", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.work.open = true;
      state.work.scope = "all";
      state.history = [{
        at: 3, who: "Claude", sessionKey: "s1", said: "make it blue", count: 0, mine: true,
        view: { open: true, shown: new Set() },
        answer: { sessionKey: "s1", who: "Claude", turns: [
          { said: summaryFor([], "build", "make it blue", null), mine: true },
          { said: "Done.", mine: false },
        ] },
      }];
      render();
    `);
    const thread = page.dom.document.querySelector('[data-entry="s1"] .work-thread');
    expect(thread?.querySelector('p[data-mine="true"]'), "the duplicate opening turn is skipped").toBeNull();
    expect(thread?.querySelectorAll(".work-line").length).toBe(1);
  });

  /** A read transcript: four asks, the middle two of which changed files. */
  const fourAsks = `
    state.work.open = true;
    state.work.scope = "all";
    state.history = [{
      at: 3, who: "Term", sessionKey: "s1", said: "a row of its own", count: 0, mine: false,
      view: { open: true, shown: new Set() },
      answer: { sessionKey: "s1", who: "Term", fromTranscript: true, turns: [
        { id: "u0", said: "look around first", mine: true },
        { said: "Looked.", mine: false },
        { id: "u1", said: "make it red", mine: true, edits: [
          { path: "/p/a.css", kind: "edit", old: "v1", new: "v2" },
          { path: "/p/a.css", kind: "edit", old: "v2", new: "v3" },
        ] },
        { said: "Done.", mine: false },
        { id: "u2", said: "and the footer", mine: true, edits: [{ path: "/p/b.html", kind: "write" }] },
        { said: "Done too.", mine: false },
        { id: "u3", said: "thanks", mine: true },
        { said: "Welcome.", mine: false },
      ] },
    }];
    render();
  `;
  const yourTurn = (page: ReturnType<typeof openTheToolbar>, words: string) =>
    [...page.dom.document.querySelectorAll('[data-entry="s1"] .work-line')].find(
      (line) => line.querySelector('p[data-mine="true"]')?.textContent === words,
    );
  const rewindOn = (line: Element | null | undefined) =>
    [...(line?.querySelectorAll(".work-rewind-at") ?? [])].find((b) => b.textContent === "↺ Rewind to here");

  test("every You turn with edits at or after it offers Rewind to here, and the row has none of its own", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(fourAsks);
    expect(rewindOn(yourTurn(page, "look around first")), "an ask before the edits goes back past them").toBeTruthy();
    expect(rewindOn(yourTurn(page, "make it red")), "an ask that made edits").toBeTruthy();
    expect(rewindOn(yourTurn(page, "and the footer"))).toBeTruthy();
    expect(rewindOn(yourTurn(page, "thanks")), "nothing changed at or after this ask").toBeUndefined();
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    expect(row?.querySelectorAll(".work-rewind-at").length, "on Your turns only, never on Claude's").toBe(3);
    expect(button(row, "Rewind files to here"), "not once at the foot of the row").toBeUndefined();
    expect(button(row, "Copy /rewind"), "and no Copy /rewind there either").toBeUndefined();
  });

  test("Rewind to here dry-runs that ask's edits and every later one's, oldest first, then puts them back", async () => {
    const plan = {
      restored: [
        { file: "a.css", path: "/p/a.css" },
        { file: "a.css", path: "/p/a.css" },
      ],
      skipped: [{ file: "b.html", path: "/p/b.html", why: "it was a Write" }],
    };
    const page = openTheToolbar({ answers: { colai_revert_edits: plan } });
    await settle();
    page.run(fourAsks);
    click(page, rewindOn(yourTurn(page, "make it red")));
    await settle();

    const calls = page.askedWith.filter(([n]) => n === "colai_revert_edits") as [
      string,
      Record<string, unknown>,
    ][];
    expect(calls.length, "a dry run first, and nothing more").toBe(1);
    expect(calls[0][1].dryRun).toBe(true);
    // In the order they were made: the command takes them off last first itself.
    expect(calls[0][1].edits).toEqual([
      { path: "/p/a.css", kind: "edit", old: "v1", new: "v2" },
      { path: "/p/a.css", kind: "edit", old: "v2", new: "v3" },
      { path: "/p/b.html", kind: "write" },
    ]);
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    expect(row?.querySelectorAll(".work-rewind").length, "one preview").toBe(1);
    const preview = yourTurn(page, "make it red")?.querySelector(".work-rewind");
    expect(preview, "under the ask it is about").toBeTruthy();
    const said = preview?.textContent ?? "";
    expect(said).toContain("Will put back 2 edits in a.css");
    expect(said).toContain("Can't put back 1 — b.html (it was a Write)");
    expect(said).toContain("To rewind the conversation too, paste /rewind in the terminal running this chat");
    expect(button(preview, "Leave it")).toBeTruthy();
    expect(button(preview, "Copy /rewind")).toBeTruthy();

    click(page, button(yourTurn(page, "make it red")?.querySelector(".work-rewind"), "Rewind"));
    await settle();
    const real = page.askedWith.filter(([n]) => n === "colai_revert_edits") as [
      string,
      Record<string, unknown>,
    ][];
    expect(real.length, "then the real one").toBe(2);
    expect(real[1][1].dryRun).toBe(false);
    expect(real[1][1].edits, "the same edits the preview named").toEqual(calls[0][1].edits);
    const receipt = yourTurn(page, "make it red")?.querySelector(".work-rewind")?.textContent ?? "";
    expect(receipt, "the preview becomes what happened").toContain("Put back 2 edits in a.css");
    expect(receipt).toContain("Couldn't put back 1");
    expect(button(yourTurn(page, "make it red")?.querySelector(".work-rewind"), "Rewind"), "nothing left to press").toBeUndefined();
    expect(page.said().trouble, "and says it").toContain("Put back 2 edits in a.css");
    // Spent: nothing at or after that ask is offered again.
    expect(page.run("state.history[0].answer.turns.some((t) => t.edits)")).toBe(false);
    expect(page.dom.document.querySelectorAll('[data-entry="s1"] .work-rewind-at').length).toBe(0);
  });

  test("one preview at a time: opening another ask's closes the first, and pressing again puts it away", async () => {
    const page = openTheToolbar({ answers: { colai_revert_edits: { restored: [], skipped: [] } } });
    await settle();
    page.run(fourAsks);
    click(page, rewindOn(yourTurn(page, "look around first")));
    await settle();
    expect(yourTurn(page, "look around first")?.querySelector(".work-rewind")).toBeTruthy();
    click(page, rewindOn(yourTurn(page, "and the footer")));
    await settle();
    expect(page.dom.document.querySelectorAll('[data-entry="s1"] .work-rewind').length).toBe(1);
    expect(yourTurn(page, "and the footer")?.querySelector(".work-rewind")).toBeTruthy();
    const calls = page.askedWith.filter(([n]) => n === "colai_revert_edits") as [
      string,
      Record<string, unknown>,
    ][];
    expect(calls[1][1].edits, "only the later ask's edits").toEqual([{ path: "/p/b.html", kind: "write" }]);
    click(page, rewindOn(yourTurn(page, "and the footer")));
    expect(Boolean(page.dom.document.querySelector('[data-entry="s1"] .work-rewind')), "and it is put away").toBe(false);
  });

  test("a just-sent mark is rewindable off its row's own record before the transcript is read", async () => {
    const page = openTheToolbar({
      answers: { colai_revert_edits: { restored: [{ file: "a.css", path: "/p/a.css" }], skipped: [] } },
    });
    await settle();
    page.run(`
      state.work.open = true;
      state.work.scope = "all";
      state.history = [{ at: 2, who: "Term", sessionKey: "s1", said: "make it red", count: 0, mine: true,
        view: { open: true, shown: new Set() },
        changes: [{ path: "/p/a.css", kind: "edit", old: "a", new: "b" }],
        answer: { sessionKey: "s1", who: "Term", turns: [{ said: "Done.", mine: false }] } }];
      render();
    `);
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    const back = row?.querySelector(".work-thread .work-rewind-at");
    expect(back?.textContent, "on the ask the row leads with").toBe("↺ Rewind to here");
    click(page, back);
    await settle();
    const calls = page.askedWith.filter(([n]) => n === "colai_revert_edits") as [
      string,
      Record<string, unknown>,
    ][];
    expect(calls[0][1]).toEqual({ edits: [{ path: "/p/a.css", kind: "edit", old: "a", new: "b" }], dryRun: true });
    click(page, button(page.dom.document.querySelector('[data-entry="s1"] .work-rewind'), "Rewind"));
    await settle();
    expect(page.run("state.history[0].changes.length"), "the record is spent").toBe(0);

    // Once the transcript is read, it is what each ask's edits come from instead.
    page.run(`state.history[0].changes = [{ path: "/p/x.css", kind: "edit", old: "1", new: "2" }];
      state.history[0].view.rewind = null;
      state.history[0].answer.fromTranscript = true; render();`);
    expect(page.dom.document.querySelector('[data-entry="s1"] .work-rewind-at')).toBeNull();
  });

  test("Copy /rewind in the preview copies the command", async () => {
    const page = openTheToolbar({ answers: { colai_revert_edits: { restored: [], skipped: [] } } });
    await settle();
    // linkedom's window answers `navigator` itself, with no clipboard and no way to give it
    // one, so the shared copy mechanism (the code block's Copy, toolbar-markdown.js) is
    // stood in for — what matters here is what is handed to it.
    page.run(`
      mdCopy = (button, text, label) => { globalThis.copied = { text, label }; };
    `);
    page.run(fourAsks);
    click(page, rewindOn(yourTurn(page, "make it red")));
    await settle();
    click(page, button(page.dom.document.querySelector('[data-entry="s1"] .work-rewind'), "Copy /rewind"));
    expect(page.run("globalThis.copied && globalThis.copied.text")).toBe("/rewind");
    expect(page.run("globalThis.copied.label"), "and the button goes back to its own name").toBe(
      "Copy /rewind",
    );
  });

  test("a turn that settles puts its edits on its Work row", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.history = [{ at: 2, who: "Term", sessionKey: "s1", said: "do it", count: 0, mine: true,
        view: { open: false, shown: new Set() } }];
      state.turns = { s1: { kind: "answer", state: "working" } };
      noteStep({ sessionKey: "s1", id: "e1", name: "Edit", args: { file_path: "/p/a.css", old_string: "a", new_string: "b" } });
      noteStep({ sessionKey: "s1", id: "w1", name: "Write", args: { file_path: "/p/n.html", content: "<html>" } });
    `);
    page.heard["colai:response"]({ payload: { turnId: "s1", state: "done" } });
    expect(page.run("JSON.stringify(state.history[0].changes)")).toBe(
      JSON.stringify([
        { path: "/p/a.css", kind: "edit", old: "a", new: "b" },
        // A Write is kept as a name only: its whole new file is not what a rewind needs.
        { path: "/p/n.html", kind: "write", old: "", new: "" },
      ]),
    );
    // Once: a second "done" for the same turn adds nothing.
    page.heard["colai:response"]({ payload: { turnId: "s1", state: "done" } });
    expect(page.run("state.history[0].changes.length")).toBe(2);
  });

  test("the undone frame keeps the prompt the confirmation is addressed by", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.undoing = { asked: true, files: null, prompt: "colai-1" }`);
    page.heard["colai:undone"]({ payload: { files: [{ path: "/p/a.css" }] } });
    expect(page.run("state.undoing.prompt"), "an older row's undo stays that row's").toBe("colai-1");
    expect(page.run("state.undoing.files.length")).toBe(1);
  });

  test("opening a row writes the transcript into the answer that is already listening", async () => {
    const page = openTheToolbar({
      answers: { colai_said: [{ said: "hi", mine: true }, { said: "hello", mine: false }] },
    });
    await settle();
    page.run(`
      globalThis.listening = { sessionKey: "s1", who: "Term", turns: [] };
      state.answers = [listening];
      state.history = [{ at: 2, who: "Term", sessionKey: "s1", said: "hi", count: 0, mine: true,
        view: { open: false, shown: new Set() }, answer: null }];
    `);
    await page.run("loadTurns(state.history[0])");
    expect(page.run("state.history[0].answer === listening"), "the row reads the live answer").toBe(true);
    expect(page.run("listening.turns.length")).toBe(2);
    expect(page.run("state.answers.length"), "and nothing is listed twice").toBe(1);
  });
});

describe("a reply in a handed chat after its turn had ended", () => {
  /*
   * The mirror settles at the first end of a turn; the agent that said "I can't yet, I'm in
   * plan mode" and did the work three turns later was never heard from. Rust follows the
   * transcript on and sends `colai:later-reply`, one per finished reply. These stand in for
   * that frame and read back what the page did with it.
   */
  type Clickable = Element & { dispatchEvent: (e: unknown) => void };
  const click = (page: ReturnType<typeof openTheToolbar>, node: Element | null | undefined) =>
    (node as Clickable | null | undefined)?.dispatchEvent(new page.dom.window.Event("click"));
  const badge = (page: ReturnType<typeof openTheToolbar>) =>
    page.dom.document.querySelector(".send-key .send-unread") as HTMLElement | null;

  async function withAConversation() {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.work.scope = "all";
      state.history = [{
        at: 3, who: "Plan chat", sessionKey: "s1", said: "build the page", count: 0, mine: true,
        view: { open: false, shown: new Set() },
        answer: { sessionKey: "s1", who: "Plan chat", turns: [
          { said: "build the page", mine: true },
          { said: "I am in plan mode, so I cannot yet.", mine: false },
        ] },
      }];
      state.answers = [state.history[0].answer];
      render();
    `);
    return page;
  }

  test("it lands on the row, counts as unread on the Work key, and says so in a toast", async () => {
    const page = await withAConversation();
    expect(badge(page)?.hidden, "nothing unread to begin with").toBe(true);
    page.heard["colai:later-reply"]({
      payload: { sessionKey: "s1", id: "u-1", said: "## Done\n\n**Built** the page and wired the form." },
    });
    expect(page.run("state.history[0].answer.turns.length")).toBe(3);
    expect(page.run("state.history[0].answer.turns[2].said")).toContain("**Built** the page");
    expect(badge(page)?.hidden, "the badge shows").toBe(false);
    expect(badge(page)?.textContent).toBe("1");
    expect(badge(page)?.getAttribute("aria-label")).toBe("1 unread reply");
    const toast = page.dom.document.querySelector("#toasts .toast");
    expect(toast?.querySelector(".toast-who")?.textContent).toBe("Plan chat replied");
    expect(toast?.querySelector(".toast-said")?.textContent, "the first line, without markdown").toBe("Done");

    page.run("state.work.open = true; render();");
    expect(
      page.dom.document.querySelector('[data-entry="s1"] .work-new')?.textContent,
      "the row says it has something new",
    ).toBe("New");
    expect(page.run("state.unread.s1"), "opening the panel alone reads nothing").toBe(1);
  });

  test("the same reply twice is one reply", async () => {
    const page = await withAConversation();
    const frame = { payload: { sessionKey: "s1", id: "u-1", said: "Done." } };
    page.heard["colai:later-reply"](frame);
    page.heard["colai:later-reply"](frame);
    expect(page.run("state.history[0].answer.turns.length")).toBe(3);
    expect(badge(page)?.textContent).toBe("1");
    expect(page.dom.document.querySelectorAll("#toasts .toast").length).toBe(1);
  });

  test("a second reply counts as two, and one toast per conversation stays up", async () => {
    const page = await withAConversation();
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "First." } });
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-2", said: "Second." } });
    expect(badge(page)?.textContent).toBe("2");
    expect(badge(page)?.getAttribute("aria-label")).toBe("2 unread replies");
    const toasts = page.dom.document.querySelectorAll("#toasts .toast");
    expect(toasts.length, "the newer toast replaces the older").toBe(1);
    expect(toasts[0]?.querySelector(".toast-said")?.textContent).toBe("Second.");
  });

  test("a long first line is cut short for the toast", async () => {
    const page = await withAConversation();
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "word ".repeat(40) } });
    const said = page.dom.document.querySelector("#toasts .toast-said")?.textContent ?? "";
    expect(said.length).toBeLessThanOrEqual(80);
    expect(said.endsWith("…")).toBe(true);
  });

  test("expanding the row reads it, and the badge goes down", async () => {
    const page = await withAConversation();
    page.run(`
      state.history.push({ at: 2, who: "Other", sessionKey: "s2", said: "other", count: 0, mine: true,
        view: { open: false, shown: new Set() }, answer: null });
      state.work.open = true;
      render();
    `);
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "Done." } });
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s2", id: "u-2", said: "Also done." } });
    expect(badge(page)?.textContent).toBe("2");
    click(page, page.dom.document.querySelector('[data-entry="s1"] .work-ask'));
    expect(page.run("state.unread.s1 ?? 0")).toBe(0);
    expect(badge(page)?.textContent, "only the other conversation is left").toBe("1");
    expect(page.dom.document.querySelector('[data-entry="s1"] .work-new')).toBeNull();
    expect(page.dom.document.querySelector('[data-entry="s2"] .work-new')).toBeTruthy();
  });

  test("a row never read before still fetches its transcript when opened", async () => {
    const page = await withAConversation();
    page.run(`state.history[0].answer = null; state.answers = [];`);
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "Done." } });
    expect(page.run("state.history[0].answer.partial")).toBe(true);
    page.asked.length = 0;
    page.run("state.work.open = true; render();");
    click(page, page.dom.document.querySelector('[data-entry="s1"] .work-ask'));
    expect(page.asked, "one reply is not the conversation").toContain("colai_said");
  });

  test("pressing the toast opens Work on that row and reads it", async () => {
    const page = await withAConversation();
    page.run(`state.work.scope = "mine"; state.receiving = { id: "someone-else", name: "x" };`);
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "Done." } });
    click(page, page.dom.document.querySelector("#toasts .toast"));
    expect(page.run("state.work.open")).toBe(true);
    expect(page.run("state.history[0].view.open"), "the row is unfolded").toBe(true);
    expect(page.dom.document.querySelector('[data-entry="s1"]'), "and widened into view").toBeTruthy();
    expect(page.run("state.unread.s1 ?? 0")).toBe(0);
    expect(badge(page)?.hidden).toBe(true);
    expect(page.dom.document.querySelectorAll("#toasts .toast").length, "the toast has done its job").toBe(0);
  });

  test("a reply on a conversation the panel has never listed makes a row for it", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`state.history = []; state.sessions = [{ key: "new-1", title: "Fresh chat", lastPrompt: "hi" }];`);
    page.heard["colai:later-reply"]({ payload: { sessionKey: "new-1", id: "u-1", said: "Hello." } });
    expect(page.run("state.history.length")).toBe(1);
    expect(page.run("state.history[0].who")).toBe("Fresh chat");
    expect(page.run("state.history[0].answer.turns[0].said")).toBe("Hello.");
  });

  test("the card showing that conversation says the new reply", async () => {
    const page = await withAConversation();
    page.run(`
      state.turns = { s1: { kind: "answer", state: "done", body: "I am in plan mode.", steps: [] } };
      state.responding = { turnId: "s1", at: { x: 0, y: 0 }, who: "Plan chat", said: "build the page", marks: [] };
      render();
    `);
    page.heard["colai:later-reply"]({ payload: { sessionKey: "s1", id: "u-1", said: "Built it after all." } });
    expect(page.run("state.turns.s1.body")).toBe("Built it after all.");
    expect(page.dom.document.querySelector("#response")?.textContent).toContain("Built it after all.");
  });
});

/*
 * ── doing less while nothing changes ─────────────────────────────────────────
 *
 * An idle toolbar is most of a day. These are the places it used to keep working for an
 * unchanged picture: a jellyfish swimming for nobody, the rail re-measured on every frame of
 * that swim, a full redraw per frame from the Rust side, a round trip every five seconds for
 * a constant, and the Work panel's rows torn down and rebuilt by renders that had nothing to
 * do with them.
 */
describe("an idle toolbar settles", () => {
  /** The jellyfish loop's own interval, or null once it has settled. */
  const loopOf = (page: ReturnType<typeof openTheToolbar>) => {
    const id = page.run("jellyTimer") as number | null;
    return id === null ? null : page.intervals.get(id) ?? null;
  };

  test("the jellyfish lies still and stops ticking after a quiet spell", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("state.runs = []; state.atWork = null; render();");
    loopOf(page)!.fn();
    expect(page.run("jellySettled"), "still swimming straight after a render").toBe(false);
    // Nothing for longer than the settle window — moved back rather than waited out.
    page.run("jellyStirred -= JELLY_SETTLE_MS + 1000;");
    loopOf(page)!.fn();
    expect(page.run("jellySettled"), "settled").toBe(true);
    expect(page.run("jellyTimer"), "and no interval left running").toBe(null);
    const still = page.dom.document.querySelector(".home-key svg ellipse")?.getAttribute("ry");
    expect(still, "the still pose: eyes open").toBe("4.40");
  });

  test("work, a render or the pointer on the rail sets it swimming again", async () => {
    const page = openTheToolbar();
    await settle();
    const lieDown = () => {
      page.run("state.runs = []; state.atWork = null; render(); jellyStirred -= JELLY_SETTLE_MS + 1000;");
      loopOf(page)!.fn();
      expect(page.run("jellySettled")).toBe(true);
    };

    lieDown();
    page.run(`state.runs = [{ sessionKey: "s1", who: "Claude", heard: Date.now() }]; render();`);
    loopOf(page)!.fn();
    expect(page.run("jellySettled"), "work woke it").toBe(false);
    expect(loopOf(page)?.ms, "at the working pace").toBeLessThanOrEqual(40);
    // And work keeps it awake however long it runs.
    page.run("jellyStirred -= JELLY_SETTLE_MS + 1000;");
    loopOf(page)!.fn();
    expect(page.run("jellySettled"), "never settles mid-run").toBe(false);

    lieDown();
    page.run("render();");
    expect(page.run("jellySettled"), "a render woke it").toBe(false);

    lieDown();
    page.dom.document.getElementById("rail-wrap")!.dispatchEvent(new page.dom.window.Event("pointerenter"));
    expect(page.run("jellySettled"), "the pointer woke it").toBe(false);
    expect(loopOf(page), "with its interval back").toBeTruthy();
  });

  test("reduced motion stays still, and settles too", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`matchMedia = () => ({ matches: true }); state.runs = []; render();`);
    loopOf(page)!.fn();
    expect(page.dom.document.querySelector(".home-key svg ellipse")?.getAttribute("ry")).toBe("4.40");
    page.run("jellyStirred -= JELLY_SETTLE_MS + 1000;");
    loopOf(page)!.fn();
    expect(page.run("jellyTimer")).toBe(null);
  });

  test("the rail's observer ignores the swim and its own render, but not a real change", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      var shapes = 0;
      var shapeItself = shape;
      shape = function () { shapes++; return shapeItself(); };
    `);
    const path = "document.querySelector('.home-key svg path')";
    page.run(`railChanged([{ target: ${path} }, { target: ${path}.parentNode }]);`);
    expect(page.run("shapes"), "a frame of the swim is not a change to the rail").toBe(0);
    page.run(`render(); shapes = 0; railChanged([{ target: el.wrap }]);`);
    expect(page.run("shapes"), "render already shaped in this task").toBe(0);
    await settle();
    page.run(`railChanged([{ target: el.wrap }, { target: ${path} }]);`);
    expect(page.run("shapes"), "a change to the rail itself is measured").toBe(1);
  });

  test("frames from the Rust side are drawn once per frame, not once each", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      var renders = 0;
      var renderItself = render;
      render = function () { renders++; return renderItself(); };
    `);
    for (let i = 1; i <= 3; i++) page.heard["colai:spent"]({ payload: { spent: i } });
    expect(page.run("renders"), "nothing drawn inside the burst").toBe(0);
    await settle();
    expect(page.run("renders"), "one drawing for three frames").toBe(1);
    expect(page.run("state.spent")).toBe(3);
  });

  test("a reply with no pin waiting still moves the line beside the mark", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      state.answers = [];
      state.doing = null;
      state.atWork = { running: 1, waiting: 0, trouble: 0, working: ["s1"], troubled: [], known: ["s1"] };
      render();
    `);
    const before = page.run("el.doing.textContent") as string;
    page.heard["colai:reply"]({
      payload: { sessionKey: "s1", message: { role: "assistant", content: "Reading the stylesheet to find the colour." } },
    });
    await settle();
    const now = page.run("el.doing.textContent") as string;
    expect(page.run("state.doing && state.doing.said"), "heard").toBeTruthy();
    expect(page.run("el.doing.hidden")).toBe(false);
    expect(now, "and drawn").toBe(page.run("state.doing.said"));
    expect(now).not.toBe(before);
  });

  test("the 5s watch no longer asks colai_at_work, and the light reads its empty answer", async () => {
    const page = openTheToolbar();
    await settle();
    page.run("watchEverything()");
    await settle();
    expect(page.asked).not.toContain("colai_at_work");
    expect(page.run("JSON.stringify(state.atWork)")).toBe(
      JSON.stringify({ running: 0, waiting: 0, trouble: 0, working: [], troubled: [], known: [] }),
    );
  });

  test("a hidden subtree and a picture's insides are not walked when measuring", async () => {
    const page = openTheToolbar();
    await settle();
    page.run(`
      var looked = [];
      var styleItself = getComputedStyle;
      // Everything visible, so the walk goes as deep as a real page's would.
      getComputedStyle = function (node) {
        looked.push(node);
        return { ...styleItself(node), overflowX: "visible", overflowY: "visible", pointerEvents: "auto" };
      };
      boxAround(el.wrap);
    `);
    expect(
      page.run("looked.some((node) => node.closest && node.closest('[hidden]') && el.wrap.contains(node))"),
      "nothing under a hidden element",
    ).toBe(false);
    expect(
      page.run("looked.some((node) => node.parentNode && node.parentNode.closest && node.parentNode.closest('svg'))"),
      "nothing inside an svg",
    ).toBe(false);
    expect(page.run("looked.some((node) => String(node.tagName).toLowerCase() === 'svg')"), "the svg itself is").toBe(true);
  });
});

describe("the Work panel is rebuilt only when its rows change", () => {
  const CHATS = [
    { key: "s1", title: "First chat", lastPrompt: "build the page", preview: "proj", at: 1_700_000_000_000 },
    { key: "s2", title: "Second chat", lastPrompt: "fix the form", preview: "proj", at: 1_699_999_000_000 },
  ];

  async function openPanel() {
    const answers: Record<string, unknown> = { colai_sessions: CHATS.map((one) => ({ ...one })) };
    const page = openTheToolbar({ answers });
    await settle();
    page.run(`state.work.scope = "all"; state.work.open = true; render();`);
    const builds = () => page.run("workBuilds") as number;
    expect(page.dom.document.querySelectorAll("#work .work-turn").length, "both rows drawn").toBe(2);
    return { page, builds, answers };
  }

  test("a render about something else leaves the rows standing and redraws only the composer", async () => {
    const { page, builds } = await openPanel();
    const row = page.dom.document.querySelector('[data-entry="s1"]');
    const composer = page.dom.document.querySelector("#work .work-tail");
    const was = builds();
    page.run("state.spent = 4; render(); render();");
    expect(builds(), "no rebuild").toBe(was);
    expect(page.dom.document.querySelector('[data-entry="s1"]'), "the very same row").toBe(row);
    expect(page.dom.document.querySelector("#work .work-tail"), "the composer is drawn fresh").not.toBe(composer);
    expect(page.dom.document.querySelector("#work .work-write"), "and is still there").toBeTruthy();
  });

  test("each thing that changes a row rebuilds it", async () => {
    const { page, builds, answers } = await openPanel();
    let was = builds();
    const rebuilt = (why: string) => {
      expect(builds(), why).toBe(was + 1);
      was = builds();
    };

    // The person: unfolding a row.
    (page.dom.document.querySelector('[data-entry="s2"] .work-ask') as unknown as { click(): void }).click();
    rebuilt("a row unfolded");
    // The scope.
    page.run(`state.work.scope = "mine"; render();`);
    rebuilt("the scope changed");
    page.run(`state.work.scope = "all"; render();`);
    rebuilt("and back");
    // A reply after the fact, and reading it.
    page.run(`heardLaterReply({ sessionKey: "s1", id: "late-1", said: "Done." }); render();`);
    rebuilt("a later reply landed");
    page.run(`markSeen("s1"); render();`);
    rebuilt("it was read");
    // A run starting.
    page.run(`state.runs = [{ sessionKey: "s1", who: "First chat", heard: Date.now() }]; render();`);
    rebuilt("a run started");
    // A live reply into a waiting answer.
    page.run(`state.answers = [state.history.find((one) => one.sessionKey === "s1").answer];`);
    page.heard["colai:reply"]({
      payload: { sessionKey: "s1", message: { role: "assistant", content: [{ type: "text", text: "More." }] } },
    });
    await settle();
    rebuilt("a live reply landed");
    // The list itself changing.
    answers.colai_sessions = [
      ...CHATS,
      { key: "s3", title: "Third", lastPrompt: "x", preview: "p", at: 1_699_000_000_000 },
    ];
    page.run("loadWork()");
    await settle();
    rebuilt("a new conversation was listed");
  });

  test("a transcript ticking within the same minute is not a change", async () => {
    const { page, builds, answers } = await openPanel();
    const was = builds();
    page.run(`
      var renders = 0;
      var renderItself = render;
      render = function () { renders++; return renderItself(); };
    `);
    const renders = () => page.run("renders") as number;
    // Kept inside one minute: 1_700_000_000_000 is 20s into its minute.
    answers.colai_sessions = CHATS.map((one) => (one.key === "s1" ? { ...one, at: one.at + 5_000 } : { ...one }));
    page.run("loadWork()");
    await settle();
    expect(renders(), "five seconds of activity redraws nothing").toBe(0);
    expect(builds()).toBe(was);
    answers.colai_sessions = CHATS.map((one) => (one.key === "s1" ? { ...one, at: one.at + 120_000 } : { ...one }));
    page.run("loadWork()");
    await settle();
    expect(renders(), "two minutes later is news").toBe(1);
    expect(builds()).toBe(was + 1);
  });
});
