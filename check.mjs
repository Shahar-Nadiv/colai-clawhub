/* What has to be true about the page before it is pushed.
 *
 * Written after breaking it twice the same way: editing this file by splicing at a matched
 * string, where the match was not the one intended. The first time it duplicated a whole
 * block; the second it wedged the bottom half of the stylesheet inside a media query, so
 * every one of those rules applied below 900px only — the page looked finished on a phone
 * and unstyled on a desktop, which is a confusing enough symptom to waste an afternoon on.
 *
 * Both were a second of arithmetic away from being obvious. So:
 *
 *     node check.mjs
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const html = readFileSync(join(here, "index.html"), "utf8");
const css = html.slice(html.indexOf("<style>") + 7, html.indexOf("</style>"));

const wrong = [];
const check = (ok, said) => { if (!ok) wrong.push(said); };

// ── the stylesheet closes everything it opens ────────────────────────────────
let depth = 0;
let balancedAt = 0;
css.split("\n").forEach((line, i) => {
  depth += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length;
  if (depth === 0) balancedAt = i + 1;
  if (depth < 0) check(false, `an extra } around stylesheet line ${i + 1}`);
});
check(depth === 0, `${depth} unclosed rule(s); last balanced at stylesheet line ${balancedAt}`);

// ── nothing is declared twice ────────────────────────────────────────────────
//
// A duplicated block is invisible in a browser — the second copy simply wins — so the page
// looks right while carrying two of everything, until one copy is edited and the other is not.
const selectors = [...css.matchAll(/^(\.[\w-]+|footer|h1|body|html)\{/gm)].map((m) => m[1]);
const twice = selectors.filter((s, i) => selectors.indexOf(s) !== i);
check(twice.length === 0, `declared more than once at the top level: ${[...new Set(twice)].join(", ")}`);

// ── the layout rules are not trapped in a media query ────────────────────────
//
// The actual bug: a rule spliced inside `@media (max-width: …)` still parses, still applies,
// and applies at exactly the wrong sizes.
const firstMedia = css.indexOf("@media");
for (const rule of [".where{", ".where-list{", ".when{", "footer{", ".card{", ".film{"]) {
  const at = css.indexOf("\n" + rule);
  check(at !== -1 && at < firstMedia, `${rule.slice(0, -1)} is missing or trapped inside a media query`);
}

// ── every class the page uses has styles, and the reverse ────────────────────
const used = new Set();
for (const m of html.matchAll(/class="([^"]+)"/g)) m[1].split(/\s+/).forEach((c) => used.add(c));
// Classes the script puts on, which the markup never mentions — the chips are built at run
// time, so looking only at the HTML declares them orphans.
const script = readFileSync(join(here, "waitlist.js"), "utf8");
for (const m of script.matchAll(/className = "([^"]+)"/g)) m[1].split(/\s+/).forEach((c) => used.add(c));
const styled = new Set([...css.matchAll(/^\.([\w-]+)/gm)].map((m) => m[1]));
// `in` is applied by inline animation-delay markup and `hp` is the honeypot; both are real.
const orphan = [...styled].filter((c) => !used.has(c) && !["in", "hp"].includes(c));
check(orphan.length === 0, `styled but never used: ${orphan.join(", ")}`);

// ── the tags close ───────────────────────────────────────────────────────────
const VOID = new Set(["meta","link","br","hr","img","input","source","path","use","rect","circle","area","base","col","embed","param","track","wbr"]);
const open = [];
for (const m of html.matchAll(/<(\/?)([a-zA-Z][\w-]*)([^>]*?)(\/?)>/g)) {
  const [, closing, raw, , selfClosed] = m;
  const name = raw.toLowerCase();
  if (VOID.has(name) || selfClosed) continue;
  if (closing) {
    if (open[open.length - 1] === name) open.pop();
    else check(false, `</${name}> closes nothing`);
  } else open.push(name);
}
check(open.length === 0, `never closed: ${open.join(", ")}`);

// ── the script can find what it reaches for ──────────────────────────────────
const ids = new Set([...html.matchAll(/id="([^"]+)"/g)].map((m) => m[1]));
const wants = new Set([
  ...[...readFileSync(join(here, "waitlist.js"), "utf8").matchAll(/getElementById\("([^"]+)"\)/g)].map((m) => m[1]),
  ...[...html.matchAll(/getElementById\("([^"]+)"\)/g)].map((m) => m[1]),
]);
const missing = [...wants].filter((id) => !ids.has(id));
check(missing.length === 0, `the script looks for ids the page does not have: ${missing.join(", ")}`);

if (wrong.length > 0) {
  console.error("the page is not ready:\n" + wrong.map((w) => "  ✗ " + w).join("\n"));
  process.exit(1);
}
console.log("the page holds together.");
