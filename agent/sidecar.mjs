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
import { query } from "@anthropic-ai/claude-agent-sdk";

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

/** Start the session on first use, not at launch — a toolbar nobody has spoken to costs nothing. */
function sessionNow(options) {
  if (running) {
    return running;
  }
  running = query({
    prompt: speaking(),
    options: {
      // Every tool Claude Code has. The toolbar is a way of talking to it, not a
      // narrower thing that happens to look like it.
      permissionMode: options.permissionMode ?? "default",
      ...(CLAUDE ? { pathToClaudeCodeExecutable: CLAUDE } : {}),
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
        for (const block of message.message.content ?? []) {
          // What it is doing, while it does it — the rail's "doing" pill.
          if (block.type === "tool_use") {
            say({ event: "doing", tool: block.name });
          }
          if (block.type === "text" && block.text.trim()) {
            say({ event: "reply", said: block.text, session });
          }
        }
        continue;
      }
      if (message.type === "result") {
        say({
          event: "done",
          session,
          outcome: message.subtype,
          ms: message.duration_ms,
          // Shown, not swallowed: on this host every mark costs the person who sent it,
          // which was never true of the Gateway.
          cost: message.total_cost_usd ?? 0,
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
    sessionNow(params.options ?? {});
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
    return { session, live: Boolean(running) };
  },
};

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
  try {
    say({ id: asked.id, ok: true, result: await method(asked.params ?? {}) });
  } catch (trouble) {
    say({ id: asked.id, ok: false, error: String(trouble?.message ?? trouble) });
  }
});

// The toolbar going away takes the session with it.
lines.on("close", () => process.exit(0));
