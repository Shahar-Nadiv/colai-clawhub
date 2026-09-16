// @ts-nocheck — this file is JavaScript.
//
// A Deno Deploy playground names its entry `main.ts`, so pasting this in means Deno
// type-checks it in strict mode: every parameter here reads as an implicit `any`, and
// `counted` infers `n: null` and then refuses the number assigned to it. None of that is
// a fault in the code — it is a .js file in a .ts slot. One line is cheaper than
// annotating a file that has no types to begin with, and it is inert on Cloudflare.

/* The only piece that holds a secret.
 *
 * The waitlist page is static and public, so it can hold no credential — anything in it is
 * readable by anyone who views source. This sits between the page and a PRIVATE repository:
 * the page posts here, this commits one JSON file per enrollment, and the page never sees
 * the token, the repository, or anybody else's address.
 *
 *   GET  /   → { count }
 *   POST /   → { position, count, repeat }
 *
 * Configuration, however the host supplies it:
 *   GITHUB_TOKEN     secret. Fine-grained, scoped to the data repo alone, contents: write.
 *   DATA_REPO        "owner/repo" of the private repository.
 *   ALLOWED_ORIGIN   the site permitted to post here.
 *
 * ONE FILE, ON PURPOSE. Paste the whole thing into a Deno Deploy playground in the browser
 * and it is the entire program — no CLI, no login on your machine, nothing to install. The
 * same file also runs on Cloudflare, where `Deno` does not exist and the guard at the bottom
 * simply does not fire.
 *
 * It is plain Web-standard code — Request, Response, fetch, crypto.subtle — which is why it
 * needs no adapting for either.
 */

const API = "https://api.github.com";
const DIR = "enrollments";
const PLATFORMS = ["OpenClaw", "Claude Code", "Cursor", "Something else"];
const MAX_WORDS = 200;

/* The count is read from a directory listing, which is a round trip to GitHub. Held briefly
 * so that a page everybody is looking at does not spend a request each. Per isolate, so it
 * is a damper rather than a cache — being a few seconds stale is fine for a number that
 * only ever goes up.
 *
 * Only ever for DISPLAY. A position is a claim about where somebody stands, so it is counted
 * fresh every time: a cached number hands the same position to two people who arrive in the
 * same half-minute. */
let counted = { at: 0, n: null };
const COUNT_TTL_MS = 30_000;

export async function handle(request, env) {
  const origin = request.headers.get("Origin") || "";
  // Whatever was configured, reduced to an origin.
  //
  // A browser sends `Origin: https://host` with no path, ever. But the natural thing to paste
  // into a variable named ALLOWED_ORIGIN is the address of the site — and for a project page
  // that address has a path on the end, and the bare host 404s, which makes pasting the full
  // URL look obviously right. It then matches nothing, and the failure surfaces as a CORS
  // error with no explanation. Cheaper to accept both than to be correct and unhelpful.
  const allowed = asOrigin(env.ALLOWED_ORIGIN);
  const cors = {
    // Unset means "from anywhere", and that has to hold for CORS as well as for the check
    // below. An empty header value is not permissive, it is malformed: the browser drops the
    // response and the form fails with a CORS error rather than a refusal it could explain.
    "Access-Control-Allow-Origin": allowed || "*",
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
  const reply = (body, status) =>
    new Response(JSON.stringify(body), {
      status: status || 200,
      headers: { "Content-Type": "application/json", "Cache-Control": "no-store", ...cors },
    });

  if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });

  // Named, not merely reported. "Not configured" sends somebody back through every step;
  // saying which variable is empty ends it. Names only — never a value.
  env = { ...env, DATA_REPO: asRepo(env.DATA_REPO) };
  const missing = ["GITHUB_TOKEN", "DATA_REPO"].filter((name) => !env[name]);
  if (missing.length > 0) {
    return reply(
      { error: `This receiver is not configured yet: ${missing.join(" and ")} is not set.` },
      503
    );
  }

  if (request.method === "GET") {
    try {
      return reply({ count: await countEnrollments(env) });
    } catch (trouble) {
      return reply({ error: `Couldn't read the list — ${trouble.message}` }, 502);
    }
  }

  if (request.method !== "POST") return reply({ error: "method not allowed" }, 405);

  // Not a security boundary — anything can forge an Origin — but it stops a stray page
  // somewhere else posting into this list by accident or mischief.
  if (allowed && origin && origin !== allowed) {
    return reply({ error: "not allowed from here" }, 403);
  }

  let sent;
  try {
    sent = await request.json();
  } catch {
    return reply({ error: "expected JSON" }, 400);
  }

  /* The honeypot: a field no person can see and every naive bot fills in. Answered with a
   * plausible success and nothing written, so whatever filled it learns nothing and does
   * not come back to try a different shape. */
  if (typeof sent.hp === "string" && sent.hp.trim() !== "") {
    const n = (await countEnrollments(env, { fresh: true }).catch(() => 0)) || 0;
    return reply({ position: n + 1, count: n + 1, repeat: false });
  }

  const email = String(sent.email || "").trim().toLowerCase();
  if (email.length > 254 || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return reply({ error: "That address doesn't look right." }, 400);
  }

  const wish = clampWords(String(sent.wish || "").trim(), MAX_WORDS).slice(0, 4000);
  const platforms = Array.isArray(sent.platforms)
    ? sent.platforms.filter((p) => PLATFORMS.includes(p)).slice(0, PLATFORMS.length)
    : [];

  /* Named for a hash of the address, not for the address and not for the clock.
   *
   * One file per enrollment means two people signing up at once never touch the same path,
   * so there is no read-modify-write and nothing to lose. Hashing makes the name stable,
   * which is what turns a second submission into an update of one known file rather than a
   * scan of every file looking for a duplicate — and it keeps the address out of the
   * filename, so a directory listing discloses nothing on its own. */
  const key = await sha256Hex(email);
  const path = `${DIR}/${key}.json`;

  const existing = await getFile(env, path);
  if (existing) {
    // Already on the list. That is what they wanted, so it is not an error — give back the
    // place they already hold rather than moving them to the back.
    const n = (await countEnrollments(env, { fresh: true }).catch(() => null)) ?? existing.position ?? 0;
    return reply({ position: existing.position ?? null, count: n, repeat: true });
  }

  /* The reason, carried out rather than caught and dropped.
   *
   * The same swallowing the write had, in the function beside it: a thrown message naming
   * the repository and the status, replaced at the last moment with "try again in a moment"
   * — which is advice rather than information, and is wrong whenever the cause is a token
   * rather than a blip. */
  let n;
  try {
    n = await countEnrollments(env, { fresh: true });
  } catch (trouble) {
    return reply({ error: `Couldn't reach the list — ${trouble.message}` }, 502);
  }
  const position = n + 1;

  const record = {
    email,
    wish,
    platforms,
    position,
    at: new Date().toISOString(),
    source: origin || null,
    country: request.headers.get("CF-IPCountry") || null,
  };

  const refused = await putFile(env, path, record, `waitlist: enrollment #${position}`);
  if (refused) {
    return reply({ error: `Couldn't save that — GitHub said: ${refused}` }, 502);
  }

  counted = { at: Date.now(), n: position };
  return reply({ position, count: position, repeat: false });
}

export default { fetch: handle };

/* The last line, and the only part that is host-specific.
 *
 * Pasted into a playground this starts the server; imported anywhere else it does nothing.
 * One file that is both a program and a module, which is what removes the CLI from the
 * setup entirely. */
if (typeof Deno !== "undefined" && Deno.serve) {
  // One at a time rather than `Deno.env.toObject()`: Deno Deploy does not reliably implement
  // the bulk read, and an empty object here looks exactly like unset variables.
  Deno.serve((request) =>
    handle(request, {
      GITHUB_TOKEN: Deno.env.get("GITHUB_TOKEN"),
      DATA_REPO: Deno.env.get("DATA_REPO"),
      ALLOWED_ORIGIN: Deno.env.get("ALLOWED_ORIGIN"),
    })
  );
}

/* ── GitHub ────────────────────────────────────────────────────────────────── */

function headers(env) {
  return {
    Authorization: `Bearer ${env.GITHUB_TOKEN}`,
    Accept: "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    // GitHub refuses a request with no User-Agent.
    "User-Agent": "colai-waitlist-worker",
  };
}

async function countEnrollments(env, opts) {
  const fresh = opts && opts.fresh;
  if (!fresh && counted.n !== null && Date.now() - counted.at < COUNT_TTL_MS) return counted.n;
  const res = await fetch(`${API}/repos/${env.DATA_REPO}/contents/${DIR}`, {
    headers: headers(env),
  });
  /* Before the first enrollment the directory does not exist, and that is a count of zero
   * rather than a failure. But GitHub also answers 404 for a repository a token cannot see —
   * wrong name, wrong account, repo not selected on the token — and those two 404s are the
   * same bytes. Reporting zero for both is how a completely unconfigured receiver came to
   * look healthy, right up until the first person tried to join. So the repository itself is
   * asked about before the absence is believed. */
  if (res.status === 404) {
    const repo = await fetch(`${API}/repos/${env.DATA_REPO}`, { headers: headers(env) });
    if (!repo.ok) {
      throw new Error(
        `cannot reach ${env.DATA_REPO} (${repo.status}) — check DATA_REPO and that the token selects that repository`
      );
    }
    counted = { at: Date.now(), n: 0 };
    return 0;
  }
  if (!res.ok) {
    const said = await res.json().catch(() => null);
    throw new Error(
      `GitHub said ${res.status} ${(said && said.message) || "with no reason"} for ${env.DATA_REPO}`
    );
  }
  const rows = await res.json();
  const n = Array.isArray(rows) ? rows.filter((r) => r.type === "file").length : 0;
  counted = { at: Date.now(), n };
  return n;
}

async function getFile(env, path) {
  const res = await fetch(`${API}/repos/${env.DATA_REPO}/contents/${path}`, {
    headers: headers(env),
  });
  if (!res.ok) return null;
  const body = await res.json();
  try {
    return JSON.parse(atob(String(body.content || "").replace(/\n/g, "")));
  } catch {
    return null;
  }
}

async function putFile(env, path, record, message) {
  const res = await fetch(`${API}/repos/${env.DATA_REPO}/contents/${path}`, {
    method: "PUT",
    headers: { ...headers(env), "Content-Type": "application/json" },
    body: JSON.stringify({
      message,
      content: toBase64(JSON.stringify(record, null, 2) + "\n"),
    }),
  });
  if (res.status === 201 || res.status === 200) return null;
  /* GitHub's own words, carried out. They are short and specific — "Resource not accessible
   * by personal access token" is a permission, "Not Found" is the repository — and none of
   * them quote the token. A bare "couldn't save that" costs a round trip through every part
   * of the setup to learn what one line already knew. */
  const said = await res.json().catch(() => null);
  return `${res.status} ${(said && said.message) || "no reason given"}`;
}

/* ── small things ──────────────────────────────────────────────────────────── */

async function sha256Hex(text) {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function toBase64(text) {
  const bytes = new TextEncoder().encode(text);
  // Chunked: spreading a large array into String.fromCharCode overflows the call stack, and
  // a 200-word wish in a multi-byte script is larger than it looks.
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

function asRepo(configured) {
  /* "owner/repo", however it was written down.
   *
   * The address of a repository, to anybody looking at one, is what the browser is showing:
   * https://github.com/owner/repo. Pasted into DATA_REPO that lands inside an API path and
   * asks for `/repos/https://github.com/owner/repo`, which is a 404 — a name error wearing
   * the costume of a permissions problem. The same forgiveness ALLOWED_ORIGIN gets, and for
   * the same reason: the wrong-looking value is the one a person would reasonably write.
   */
  return String(configured || "")
    .trim()
    .replace(/^https?:\/\/(?:www\.)?github\.com\//i, "")
    .replace(/\.git$/i, "")
    .replace(/^\/+|\/+$/g, "");
}

function asOrigin(configured) {
  const text = String(configured || "").trim();
  if (!text) return "";
  try {
    return new URL(text).origin;
  } catch {
    // Not a URL at all — a bare host, say. Left as written rather than guessed at, so a
    // mismatch stays visible instead of being silently widened.
    return text.replace(/\/+$/, "");
  }
}

function clampWords(text, max) {
  const parts = text.split(/\s+/).filter(Boolean);
  return parts.length <= max ? text : parts.slice(0, max).join(" ");
}
