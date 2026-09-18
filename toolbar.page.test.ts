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

const ORDER = ["toolbar-tools.js","toolbar-rail.js","toolbar-mark.js","toolbar-live.js",
  "toolbar-answers.js","toolbar-compose.js","toolbar-work.js",
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
      invoke: async (name: string) => {
        asked.push(name);
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

  return { dom, asked, trouble, at, run, said, box: named_, type, press, menu };
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
      // Two masked fills: the tentacles and the dome.
      // Both fills, and both in colai's red rather than the key's mood colour.
      expect(home?.querySelectorAll("rect.colai-ink").length).toBe(2);
      expect(home?.innerHTML, "the logo must not be repainted by the mood").not.toContain(
        "currentColor",
      );
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

describe("the four things worth knowing can be asked for again", () => {
  test("colai's own key brings the card back after it was dismissed", async () => {
    /*
     * It was shown once and there was no way back to it. Somebody who pressed "Got it" to
     * clear a card off the screen before reading it had lost the grip, the fold key, `/` and
     * `@` for the life of the install — and the markup promised the tray, which no longer
     * exists. The mark on the rail is the way back: always there, named after the
     * application, and it had nothing behind it at all.
     */
    const page = openTheToolbar();
    await settle();
    page.run("state.tips = false; render()");
    expect(page.dom.document.querySelector("#tips")?.hidden).toBe(true);

    const home = page.dom.document.querySelector(".home-key") as unknown as {
      dispatchEvent: (event: unknown) => void;
      title: string;
    };
    home.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.tips"), "pressing the mark asks for them").toBe(true);
    const card = page.dom.document.querySelector("#tips");
    expect(card?.hidden).toBe(false);
    // All four, and the two keystrokes named as working in any box rather than in one.
    expect(card?.querySelectorAll(".tips-row").length).toBe(4);
    const said = card?.textContent ?? "";
    for (const named of ["Drag the grip", "fold key", "Type / in any box", "Type @ in any box"]) {
      expect(said, `the card should still name: ${named}`).toContain(named);
    }
    // And the key says what it does, over whatever it is saying about the work.
    expect(home.title).toContain("the four things worth knowing");

    home.dispatchEvent(new page.dom.window.Event("click"));
    expect(page.run("state.tips"), "and the same press puts it away").toBe(false);
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
      "Build: Do what is asked below.\n\nfix the header",
    );
    // And with something marked, "this change" has something to point at again.
    expect(
      said('summaryFor([{ tool: "box" }], "build", "hey", null)'),
      "the instruction stands when there is a picture",
    ).toContain("Build: Make this change.");
    // The other three modes read correctly on their own and are left alone.
    expect(said('summaryFor([], "ask", "what does the rail do?", null)')).toBe(
      "Ask: Answer the question. Do not change anything yet.\n\nwhat does the rail do?",
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
