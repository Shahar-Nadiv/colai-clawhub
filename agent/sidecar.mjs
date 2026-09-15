// The toolbar's end of a Claude Code session.
//
// colai was built against OpenClaw, which runs a Gateway: a long-lived WebSocket the
// toolbar sends marks into and reads replies out of. Claude Code has no such thing. What it
// has is the Agent SDK, which spawns the `claude` executable the user is already logged
// into and drives it — so the toolbar owns the session rather than connecting to one.
//
// The SDK is Node and the toolbar is Rust, so this runs as a child process and speaks
// newline-delimited JSON on stdin and stdout. That is deliberately the same shape the
// Gateway had — requests with ids, replies with the same id, and unsolicited events — so
// the Rust and the whole UI above it barely change.
//
//     in   {"id":1,"method":"send","params":{…}}
//     out  {"id":1,"ok":true,"result":{…}}          ← an answer
//     out  {"event":"reply","said":"…"}             ← something that just happened
//
// One line per message, because a partial line is a parse error rather than a hang, and
// because `readline` gives us framing for free.
//
// What this file must never do: decide anything about what the user marked. Composition,
// redaction and the `<observed>` fence all live in `toolbar/ui/toolbar-tools.js` and arrive
// here already done. This is transport.

import { existsSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { listSessions, query } from "@anthropic-ai/claude-agent-sdk";

/**
 * The `claude` this machine already has.
 *
 * The SDK will happily use its own copy — it ships one per platform through optional
 * dependencies, the same arrangement colai uses for the toolbar binary. Two reasons not to
 * let it:
 *
 * The copy is about 200 MB, and shipping a second Claude Code beside the one the user
 * installed is a strange thing to ask of somebody who installed this *for* Claude Code.
 *
 * More importantly it would be a *different* Claude Code — its own version, and its own
 * idea of who is logged in. The whole reason this host needs no API key is that it drives
 * the binary the user has already authenticated. Pointing somewhere else quietly gives up
 * that property.
 *
 * `COLAI_CLAUDE` overrides, for anyone running a build that is not on PATH.
 */
function claudeHere() {
  const said = process.env.COLAI_CLAUDE;
  if (said) {
    return existsSync(said) ? said : null;
  }
  /*
   * PATH is walked here rather than asked of a shell.
   *
   * `sh -lc "command -v claude"` is the obvious way and it is wrong twice. It is a *login*
   * shell, so it sources the user's rc files — on this machine that failed outright on a
   * dangling snap env file and returned nothing, which this function would have read as
   * "no claude installed". And a toolbar launched from a desktop session has no business
   * running somebody's shell startup at all.
   *
   * Walking the list is fewer moving parts and cannot be broken by a config file.
   */
  for (const dir of (process.env.PATH ?? "").split(":").filter(Boolean)) {
    const here = join(dir, "claude");
    if (!existsSync(here)) {
      continue;
    }
    try {
      // Resolved, because what is on PATH is usually a shim and the SDK wants the program.
      return realpathSync(here);
    } catch {
      return here;
    }
  }
  return null;
}

const CLAUDE = claudeHere();

/** Whatever the toolbar has said that the session has not yet consumed. */
const waiting = [];
/** Woken when something joins the queue, so the generator sleeps rather than spins. */
let nudge = null;

function say(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

/**
 * The session's side of the conversation.
 *
 * An async generator, because that is what `query` takes for a session that stays open:
 * each `yield` is a turn, and awaiting between them is what keeps the session alive rather
 * than ending it. A generator that returns is a session that closes.
 */
async function* speaking() {
  for (;;) {
    while (waiting.length === 0) {
      await new Promise((wake) => {
        nudge = wake;
      });
    }
    yield waiting.shift();
  }
}

/**
 * One mark, as the Messages API wants it.
 *
 * Images first, sentence last. The order is not cosmetic: the text refers to the pictures
 * ("the thing I boxed"), and a reference that arrives before its referent reads as a
 * question about nothing.
 */
function asUserMessage({ text, images = [] }) {
  const content = images.map((image) => ({
    type: "image",
    source: { type: "base64", media_type: image.mediaType ?? "image/png", data: image.data },
  }));
  content.push({ type: "text", text: text ?? "" });
  return { type: "user", parent_tool_use_id: null, message: { role: "user", content } };
}

let session = null;
let running = null;
/** Which session the toolbar last asked to speak to, so a switch can be noticed. */
let addressed = null;
/** What this session has cost so far, in dollars. Every mark on this host costs the user. */
let spent = 0;

/**
 * Start the session on first use, not at launch — a toolbar nobody has spoken to costs
 * nothing — and start a *different* one when the toolbar addresses a different session.
 *
 * One live session at a time. Claude Code sessions are files on disk, not connections, so
 * switching is cheap: end this query, start another with `resume`. Holding several open at
 * once would buy nothing — the toolbar can only be pointed at one of them — and would mean
 * several `claude` processes for a person who thinks they are running none.
 */
function sessionNow(options, wanted) {
  if (running && wanted === addressed) {
    return running;
  }
  if (running && wanted !== addressed) {
    // Let the old generator finish so its child exits rather than being orphaned.
    waiting.length = 0;
    running = null;
    session = null;
    spent = 0;
  }
  addressed = wanted ?? null;
  running = query({
    prompt: speaking(),
    options: {
      // Every tool Claude Code has. The toolbar is a way of talking to it, not a
      // narrower thing that happens to look like it.
      permissionMode: options.permissionMode ?? "default",
      ...(CLAUDE ? { pathToClaudeCodeExecutable: CLAUDE } : {}),
      // Picking a session from the rail continues it rather than starting beside it.
      ...(wanted ? { resume: wanted } : {}),
      ...(options.model ? { model: options.model } : {}),
      ...(options.cwd ? { cwd: options.cwd } : {}),
    },
  });
  void readingReplies();
  return running;
}

/** Everything the session says, turned into events the toolbar already knows how to draw. */
async function readingReplies() {
  try {
    for await (const message of running) {
      if (message.type === "system" && message.subtype === "init") {
        session = message.session_id;
        say({
          event: "ready",
          session,
          model: message.model,
          credential: message.apiKeySource,
          claude: CLAUDE,
        });
        continue;
      }
      if (message.type === "assistant") {
        /*
         * Forwarded whole, in the shape the page already reads.
         *
         * `spokenBy` in `toolbar-answers.js` takes a message with `role: "assistant"` and
         * a content array of `{type: "text"}` blocks — which is the Messages API shape,
         * which is exactly what the SDK hands over. So this does not translate anything:
         * re-packing it into some colai-shaped reply would be inventing a second format
         * for the same thing and a second place for them to disagree.
         */
        say({ event: "reply", sessionKey: session, message: message.message });
        for (const block of message.message.content ?? []) {
          // What it is doing, while it is doing it — the pill on the rail. Only the start
          // of a call: a result says a thing has finished, and a status one step behind
          // the work is worse than none.
          if (block.type === "tool_use") {
            say({ event: "doing", name: block.name, args: block.input ?? {}, sessionKey: session });
          }
        }
        continue;
      }
      if (message.type === "result") {
        /*
         * What that turn cost, and what the session has cost.
         *
         * Shown rather than swallowed. On OpenClaw the toolbar was a way of reaching an
         * agent somebody was already paying for however they paid for it; here every mark
         * is a metered call against the user's own account, and a tool that spends
         * somebody's money without saying so is a tool they are right to distrust. The
         * running total matters more than the turn: one mark is never the problem.
         */
        spent += message.total_cost_usd ?? 0;
        say({
          event: "done",
          sessionKey: session,
          outcome: message.subtype,
          ms: message.duration_ms,
          cost: message.total_cost_usd ?? 0,
          spent,
        });
      }
    }
  } catch (trouble) {
    say({ event: "trouble", said: String(trouble?.message ?? trouble) });
  }
}

const METHODS = {
  /** A mark, on its way. Returns once queued — the reply arrives as an event. */
  send(params) {
    sessionNow(params.options ?? {}, params.sessionKey ?? null);
    waiting.push(asUserMessage(params));
    nudge?.();
    nudge = null;
    return { queued: true };
  },

  /** Stop what it is doing without ending the session. */
  async interrupt() {
    if (!running) {
      return { stopped: false };
    }
    await running.interrupt();
    return { stopped: true };
  },

  /** Whether there is a session yet, and which. */
  status() {
    return { session, live: Boolean(running), spent };
  },

  /**
   * Every conversation on this machine, newest first.
   *
   * This is what the rail's receiver list becomes here. OpenClaw had agents, projects and
   * threads because it ran many agents on somebody's behalf; Claude Code has one Claude and
   * a pile of sessions, so the thing worth picking is which conversation to carry on.
   *
   * Grouped by `cwd` on the way out, because that is the project — the same shape the tree
   * already draws, so `drawWho` needs no new idea.
   */
  async sessions(params) {
    const found = await listSessions({ limit: params.limit ?? 200 });
    return {
      sessions: found.map((one) => ({
        sessionKey: one.sessionId,
        // `/rename` wins, then the summary, then the prompt that started it. A session
        // with none of those is untitled rather than blank.
        name: one.customTitle || one.summary || one.firstPrompt || "Untitled",
        cwd: one.cwd ?? null,
        branch: one.gitBranch ?? null,
        at: one.lastModified,
      })),
    };
  },
};

/**
 * How many requests are still being answered.
 *
 * stdin closing means the toolbar has gone, and the obvious response — exit — loses
 * whatever was still in flight. It did: `sessions` is async, and closing stdin in the same
 * breath as writing the request killed the process before `listSessions` resolved, so the
 * caller got silence rather than an answer. Anything synchronous survived, which is exactly
 * the sort of bug that looks like it works.
 */
let answering = 0;
let ended = false;

function leaveWhenQuiet() {
  if (ended && answering === 0) {
    process.exit(0);
  }
}

const lines = createInterface({ input: process.stdin });
lines.on("line", async (line) => {
  if (!line.trim()) {
    return;
  }
  let asked;
  try {
    asked = JSON.parse(line);
  } catch {
    // A malformed line has no id to answer, so there is nobody to tell but the log.
    say({ event: "trouble", said: "could not parse a request" });
    return;
  }
  const method = METHODS[asked.method];
  if (!method) {
    say({ id: asked.id, ok: false, error: `no such method: ${asked.method}` });
    return;
  }
  answering += 1;
  try {
    say({ id: asked.id, ok: true, result: await method(asked.params ?? {}) });
  } catch (trouble) {
    say({ id: asked.id, ok: false, error: String(trouble?.message ?? trouble) });
  } finally {
    answering -= 1;
    leaveWhenQuiet();
  }
});

// The toolbar going away takes the session with it — after whatever it last asked for has
// been answered, so a request and a closed pipe in the same breath still gets a reply.
lines.on("close", () => {
  ended = true;
  leaveWhenQuiet();
});
