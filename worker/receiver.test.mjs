/* The Worker, exercised against a stubbed GitHub.
 *
 * Node 18+ carries the same primitives workerd does — Request, Response, fetch, crypto.subtle,
 * btoa — so the whole flow can be run here: what it asks GitHub for, what it writes, and what
 * it answers. Nothing here touches a network or an account.
 */
import assert from "node:assert/strict";

const REPO = "someone/private-data";
const ORIGIN = "https://shahar-nadiv.github.io";
const env = { GITHUB_TOKEN: "test-token", DATA_REPO: REPO, ALLOWED_ORIGIN: ORIGIN };

let store, calls, worker;
async function stubGitHub() {
  // A new module instance each time: the Worker keeps a per-isolate count cache, and a test
  // that inherited the previous test's cache would be testing the harness, not the Worker.
  worker = (await import("./receiver.js?fresh=" + Math.random())).default;
  store = new Map();
  calls = [];
  globalThis.fetch = async (url, init = {}) => {
    const u = String(url);
    calls.push({ method: init.method || "GET", url: u, headers: init.headers, body: init.body });
    assert.match(u, /^https:\/\/api\.github\.com\/repos\//, "only GitHub is ever called");
    assert.equal(init.headers.Authorization, "Bearer test-token");
    assert.ok(init.headers["User-Agent"], "GitHub refuses a request with no User-Agent");

    // The repository itself, asked about when a listing 404s to tell "no enrollments yet"
    // apart from "this token cannot see the repo".
    if (u === `https://api.github.com/repos/${REPO}`) {
      return new Response(JSON.stringify({ full_name: REPO }), { status: 200 });
    }

    const path = u.replace(`https://api.github.com/repos/${REPO}/contents/`, "");
    if ((init.method || "GET") === "PUT") {
      const sent = JSON.parse(init.body);
      assert.ok(sent.message, "a commit needs a message");
      store.set(path, Buffer.from(sent.content, "base64").toString("utf8"));
      return new Response("{}", { status: 201 });
    }
    if (path === "enrollments") {
      const files = [...store.keys()].map((p) => ({ name: p.split("/").pop(), type: "file" }));
      return files.length
        ? new Response(JSON.stringify(files), { status: 200 })
        : new Response("{}", { status: 404 });
    }
    const held = store.get(path);
    return held
      ? new Response(JSON.stringify({ content: Buffer.from(held).toString("base64") }), { status: 200 })
      : new Response("{}", { status: 404 });
  };
}

const post = (body, origin = ORIGIN) =>
  worker.fetch(
    new Request("https://w.dev/", {
      method: "POST",
      headers: { "Content-Type": "application/json", Origin: origin },
      body: JSON.stringify(body),
    }),
    env
  );
const get = () => worker.fetch(new Request("https://w.dev/", { headers: { Origin: ORIGIN } }), env);

/* A stub that can be re-armed inside a loop, where re-importing the module is not wanted. */
function stubGitHubInline() {
  store = new Map();
  calls = [];
  globalThis.fetch = async (url, init = {}) => {
    const u = String(url);
    if (u === `https://api.github.com/repos/${REPO}`) {
      return new Response(JSON.stringify({ full_name: REPO }), { status: 200 });
    }
    const path = u.replace(`https://api.github.com/repos/${REPO}/contents/`, "");
    if ((init.method || "GET") === "PUT") {
      store.set(path, "{}");
      return new Response("{}", { status: 201 });
    }
    if (path === "enrollments") return new Response("{}", { status: 404 });
    return new Response("{}", { status: 404 });
  };
}

let failures = 0;
async function check(name, fn) {
  await stubGitHub();
  try {
    await fn();
    console.log("  ✓", name);
  } catch (e) {
    failures++;
    console.log("  ✗", name, "\n     ", e.message);
  }
}

console.log("the waitlist receiver");

await check("an empty list counts zero rather than failing", async () => {
  const body = await (await get()).json();
  assert.deepEqual(body, { count: 0 });
});

await check("a first enrollment is written and answered #1", async () => {
  const res = await post({ email: "Someone@Example.com", wish: "draw on my PCB", platforms: ["Cursor"] });
  assert.equal(res.status, 200);
  assert.deepEqual(await res.json(), { position: 1, count: 1, repeat: false });

  assert.equal(store.size, 1);
  const [path, raw] = [...store.entries()][0];
  const saved = JSON.parse(raw);
  assert.equal(saved.email, "someone@example.com", "the address is normalised before it is stored");
  assert.equal(saved.wish, "draw on my PCB");
  assert.deepEqual(saved.platforms, ["Cursor"]);
  assert.equal(saved.position, 1);
  assert.ok(saved.at, "and stamped with a time");
  assert.doesNotMatch(path, /example\.com/, "the filename must not carry the address");
  assert.match(path, /^enrollments\/[0-9a-f]{64}\.json$/);
});

await check("the second is #2", async () => {
  await post({ email: "a@example.com" });
  const body = await (await post({ email: "b@example.com" })).json();
  assert.equal(body.position, 2);
  assert.equal(store.size, 2);
});

await check("the same address again keeps its place and writes nothing new", async () => {
  await post({ email: "a@example.com" });
  await post({ email: "b@example.com" });
  const before = store.size;
  const body = await (await post({ email: "A@EXAMPLE.COM " })).json();
  assert.equal(body.repeat, true, "recognised as already on the list");
  assert.equal(body.position, 1, "and given the place it already held, not the back");
  assert.equal(store.size, before, "no second file for one person");
});

await check("the honeypot is answered plausibly and stores nothing", async () => {
  const res = await post({ email: "bot@example.com", hp: "Acme Inc" });
  assert.equal(res.status, 200, "a bot must not learn it was caught");
  assert.equal(store.size, 0, "and nothing reaches the repository");
});

await check("a bad address is refused before GitHub is touched", async () => {
  const res = await post({ email: "not-an-address" });
  assert.equal(res.status, 400);
  assert.equal(calls.length, 0, "no request is made on its behalf");
});

await check("the wish is cut to 200 words and platforms to the known set", async () => {
  await post({
    email: "long@example.com",
    wish: Array.from({ length: 500 }, (_, i) => "w" + i).join(" "),
    platforms: ["Cursor", "Nonsense", "OpenClaw"],
  });
  const saved = JSON.parse([...store.values()][0]);
  assert.equal(saved.wish.split(/\s+/).length, 200);
  assert.deepEqual(saved.platforms, ["Cursor", "OpenClaw"], "an unknown platform is dropped");
});

await check("a post from somewhere else is refused", async () => {
  const res = await post({ email: "x@example.com" }, "https://evil.example");
  assert.equal(res.status, 403);
  assert.equal(store.size, 0);
});

await check("preflight answers with the one allowed origin", async () => {
  const res = await worker.fetch(
    new Request("https://w.dev/", { method: "OPTIONS", headers: { Origin: ORIGIN } }),
    env
  );
  assert.equal(res.status, 204);
  assert.equal(res.headers.get("Access-Control-Allow-Origin"), ORIGIN);
});

await check("an allowed origin pasted with a path still matches the browser", async () => {
  // What a person naturally pastes: the address of the site, path and all.
  const env2 = { ...env, ALLOWED_ORIGIN: "https://shahar-nadiv.github.io/colai-clawhub/" };
  const res = await worker.fetch(
    new Request("https://w.dev/", {
      method: "POST",
      headers: { "Content-Type": "application/json", Origin: "https://shahar-nadiv.github.io" },
      body: JSON.stringify({ email: "p@example.com" }),
    }),
    env2
  );
  assert.equal(res.status, 200, "the path on the configured value must not refuse the page");
  assert.equal(
    res.headers.get("Access-Control-Allow-Origin"),
    "https://shahar-nadiv.github.io",
    "and the header handed back must be an origin, or the browser drops it"
  );
});

await check("with no allowed origin set, CORS still permits the page", async () => {
  // The origin check being skipped is only half of "allow anywhere". An empty
  // Access-Control-Allow-Origin is malformed, and a browser drops the response on the floor.
  const res = await worker.fetch(
    new Request("https://w.dev/", { headers: { Origin: ORIGIN } }),
    { GITHUB_TOKEN: "test-token", DATA_REPO: REPO }
  );
  assert.equal(res.headers.get("Access-Control-Allow-Origin"), "*");
});

await check("with no token it refuses, and says which variable is missing", async () => {
  const res = await worker.fetch(new Request("https://w.dev/"), { ALLOWED_ORIGIN: ORIGIN });
  assert.equal(res.status, 503);
  const { error } = await res.json();
  // Naming them is the whole point: "not configured" sends somebody back through every step.
  assert.match(error, /GITHUB_TOKEN and DATA_REPO/);
});

await check("and names only the one that is actually missing", async () => {
  const res = await worker.fetch(new Request("https://w.dev/"), {
    GITHUB_TOKEN: "test-token",
    ALLOWED_ORIGIN: ORIGIN,
  });
  const { error } = await res.json();
  assert.match(error, /DATA_REPO is not set/);
  assert.doesNotMatch(error, /GITHUB_TOKEN/, "a variable that is set must not be blamed");
  assert.doesNotMatch(error, /test-token/, "and a value must never appear in an error");
});

await check("a GitHub failure is reported, not swallowed as a success", async () => {
  globalThis.fetch = async () => new Response("{}", { status: 500 });
  const res = await post({ email: "x@example.com" });
  assert.equal(res.status, 502);
});

await check("DATA_REPO written as a github.com URL still works", async () => {
  // What is on screen when you are looking at the repository, and therefore what gets pasted.
  for (const written of [
    `https://github.com/${REPO}`,
    `https://github.com/${REPO}/`,
    `https://github.com/${REPO}.git`,
    ` ${REPO} `,
  ]) {
    stubGitHubInline();
    const res = await worker.fetch(
      new Request("https://w.dev/", {
        method: "POST",
        headers: { "Content-Type": "application/json", Origin: ORIGIN },
        body: JSON.stringify({ email: "u@example.com" }),
      }),
      { ...env, DATA_REPO: written }
    );
    assert.equal(res.status, 200, `refused when written as ${written}`);
  }
});

await check("a read failure names the repository and the status", async () => {
  // The last place a reason was being dropped: a thrown message that says which repository
  // and what GitHub answered, replaced by "try again in a moment" at the point of reply.
  globalThis.fetch = async () =>
    new Response(JSON.stringify({ message: "Bad credentials" }), { status: 401 });
  const res = await post({ email: "x@example.com" });
  assert.equal(res.status, 502);
  const { error } = await res.json();
  assert.match(error, /401/);
  assert.match(error, /Bad credentials/);
  assert.match(error, new RegExp(REPO), "and which repository it was asking about");
  assert.doesNotMatch(error, /test-token/);
});

await check("a repo the token cannot see is said so, not reported as an empty list", async () => {
  // The failure that looked healthy: GitHub answers 404 both for "no enrollments directory"
  // and for "no such repository, as far as this token is concerned".
  globalThis.fetch = async () => new Response(JSON.stringify({ message: "Not Found" }), { status: 404 });
  const res = await get();
  assert.equal(res.status, 502, "an unreachable repository is not a count of zero");
});

await check("and GitHub's own words come back when a write is refused", async () => {
  const seen = [];
  globalThis.fetch = async (url, init = {}) => {
    seen.push(init.method || "GET");
    if ((init.method || "GET") === "PUT") {
      return new Response(
        JSON.stringify({ message: "Resource not accessible by personal access token" }),
        { status: 403 }
      );
    }
    // Readable, so the flow gets as far as the write — which is the case being tested: a
    // token that can see the repository but not commit to it.
    if (String(url) === `https://api.github.com/repos/${REPO}`) {
      return new Response(JSON.stringify({ full_name: REPO }), { status: 200 });
    }
    return new Response("{}", { status: 404 });
  };
  const res = await post({ email: "x@example.com" });
  assert.equal(res.status, 502);
  const { error } = await res.json();
  assert.match(error, /403/);
  assert.match(error, /not accessible by personal access token/);
  assert.doesNotMatch(error, /test-token/, "and never the token itself");
});

console.log(failures ? `\n${failures} failed` : "\nall green");
process.exit(failures ? 1 : 0);
