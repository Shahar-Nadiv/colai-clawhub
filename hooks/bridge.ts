import type { Register } from 'claude-code'

/*
 * colai's bridge into the session it runs in.
 *
 * The toolbar leaves a mark for a conversation in the outbox, `<config>/ai.colai.toolbar/outbox/
 * <session>/<mark>/say.txt`. This module runs inside that conversation's own Claude Code session
 * — a Claude Desktop Code-tab chat, or a terminal — and submits the mark as the person's own
 * prompt: a turn of its own when the session is idle, queued behind one that is busy.
 *
 * That replaces two things that never quite fitted. The relay was a second `claude` that had to
 * be signed in separately (the desktop app's sign-in is its own), cost a turn per mark, and
 * arrived in the chat as "a message from another session". The outbox hook waited for the person
 * to type something. This is neither: the mark goes in through the session's own front door.
 *
 * Two files are the whole protocol with the toolbar:
 *   - `.bridge` beside the marks, rewritten every few seconds: "a bridge is listening here", so
 *     the toolbar hands a mark to the bridge instead of the relay, and falls back when it is stale;
 *   - `delivered` inside a mark, the receipt. The plugin API can write files but not delete or
 *     rename them, so a mark is claimed by writing it — BEFORE the submit, so a reload or a second
 *     tick can never send one mark twice. The toolbar removes a mark once it has a receipt, and
 *     the outbox hook skips (and clears) any it finds.
 */

const EVERY_MS = 1500
/** Heartbeat every this many ticks: often enough to read as live, rarely enough to be quiet. */
const BEAT_EVERY = 3

type Hooks = Parameters<Register>[0]
type Engine = Parameters<Parameters<Hooks>[2]>[0]

/** Where marks for this session wait — the toolbar's own rule for its config directory. */
async function outboxFor($: Engine): Promise<string | null> {
  const xdg = await $.env.get('XDG_CONFIG_HOME')
  const home = (await $.env.get('HOME')) || (await $.env.get('USERPROFILE'))
  const config = xdg || (home ? `${home}/.config` : null)
  if (!config) return null
  const id = await $.session.id()
  return `${config}/ai.colai.toolbar/outbox/${id}`
}

/** A folder that is a whole mark: not one still being written, nor one the relay is carrying. */
function isAMark(name: string): boolean {
  return !name.startsWith('.') && !name.endsWith('.part') && !name.endsWith('.relaying')
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    let busy = false
    let tick = 0
    $.clock.every(EVERY_MS, () => {
      if (busy) return
      busy = true
      void (async () => {
        try {
          const room = await outboxFor($)
          if (!room) return
          if (tick++ % BEAT_EVERY === 0) {
            await $.fs.write(`${room}/.bridge`, String(Date.now()))
          }
          const marks = (await $.fs.list(room))
            .filter(entry => entry.kind === 'dir' && isAMark(entry.name))
            .map(entry => entry.name)
            .sort()
          for (const mark of marks) {
            const at = `${room}/${mark}`
            if (await $.fs.exists(`${at}/delivered`)) continue
            if (!(await $.fs.exists(`${at}/say.txt`))) continue
            const text = await $.fs.read(`${at}/say.txt`)
            // Claimed first: whatever happens to the submit, this mark is never sent twice.
            await $.fs.write(`${at}/delivered`, JSON.stringify({ at: Date.now(), state: 'submitting' }))
            await $.prompt.submit({ text, asUser: true })
            await $.fs.write(`${at}/delivered`, JSON.stringify({ at: Date.now(), state: 'delivered' }))
            $.ui.toast('🪼 Colai mark delivered to this chat')
          }
        } catch {
          // No outbox yet, or a mark half gone under us: the next tick looks again.
        } finally {
          busy = false
        }
      })()
    })
    return started
  })
}
