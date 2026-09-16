# The colai waitlist

A public page, a private repository, and one small thing in between that holds the only
secret.

```
  browser                     one pasted file                private repo
  ────────                    ───────────────                ────────────
  shahar-nadiv.github.io  ──► a Deno Deploy playground  ──►  colai-waitlist-data
  (no key, ever)              (holds the GitHub token)       enrollments/<hash>.json
```

The page is static and public, so **it can hold no credential** — anything in it is readable
by anyone who views source. That is the whole reason the receiver exists. It is also why the
enrollment data cannot live in `colai-clawhub`: that repo is public, and email addresses in a
public repo is a breach and a spam magnet.

## Setting it up

Three things, all in a browser. **No command line, nothing to install, no login on your
machine** — that is what was making this hard, and none of it is necessary.

### 1. A private repository for the data — 30 seconds

New repository on GitHub, named `colai-waitlist-data`, **Private**. Don't add a README or
anything else. The receiver creates `enrollments/` on the first signup.

### 2. A token that can reach only that repository — 1 minute

GitHub → Settings → Developer settings → **Fine-grained personal access tokens** → Generate.

- **Repository access**: Only select repositories → `colai-waitlist-data`
- **Permissions**: Repository permissions → **Contents: Read and write**. Nothing else.

Copy the token. It is the only secret in this system, it can touch that one private repo and
nothing else — not `colai-clawhub`, not your account — and it never goes near the website or
this folder.

### 3. The receiver — 2 minutes, paste and go

Go to **[dash.deno.com](https://dash.deno.com)** and sign in **with GitHub**. No new account,
no new password, no email to verify.

1. **New Playground**
2. Delete the sample code, paste the whole of `worker/receiver.js`
3. **Save & Deploy**
4. Settings → **Environment Variables**, add three:

| Name | Value |
|---|---|
| `GITHUB_TOKEN` | the token from step 2 |
| `DATA_REPO` | `Shahar-Nadiv/colai-waitlist-data` |
| `ALLOWED_ORIGIN` | `https://shahar-nadiv.github.io` |

Both are forgiving of the obvious spelling. `DATA_REPO` takes the full
`https://github.com/owner/repo` too, and `ALLOWED_ORIGIN` takes the site address with its
path — each is reduced to what the API and the browser actually use. Leaving `ALLOWED_ORIGIN`
unset means "from anywhere", which is fine while you are getting it working.

If something is wrong, the receiver says which: open its URL in a browser and the `GET` reads
the list through the same path a signup does, so it reports the same diagnosis — quoting
GitHub's own words and naming the repository it asked about. It never quotes the token.

5. Copy the project's URL and put it in `config.js`:

```js
window.COLAI_WAITLIST = {
  endpoint: "https://colai-waitlist-xxxx.deno.dev",
};
```

Commit that one line and it is live. Until it is a real URL the form stays **disabled and
says so**, rather than accepting an address it has nowhere to put.

`receiver.js` is one self-contained file for exactly this reason — pasted into a playground it
is the whole program, and the last three lines are the only host-specific thing in it.

A playground calls its entry `main.ts`, so Deno type-checks what you paste. The file is
JavaScript, which strict TypeScript refuses on sight — every parameter reads as an implicit
`any`. The `// @ts-nocheck` on line 1 is what makes it paste cleanly; leave it there.

### If you would rather use Cloudflare

The same file works there (`wrangler.toml` points at it). It is not recommended from this
machine: `wrangler login` needs a browser round trip to `localhost`, and Cloudflare is reached
over IPv6 here — which routes to London rather than Haifa, and that asymmetry is what broke
the dashboard login and the OAuth callback. Deno Deploy's browser playground avoids the whole
class of problem.

## What gets stored

One file per person, at `enrollments/<sha256 of the address>.json`:

```json
{
  "email": "someone@example.com",
  "wish": "draw a path on my PCB and have it understand the trace",
  "platforms": ["Cursor", "OpenClaw"],
  "position": 7,
  "at": "2026-09-16T12:04:11.512Z",
  "source": "https://shahar-nadiv.github.io",
  "country": "GB"
}
```

Named for a hash rather than the address, so a directory listing discloses nothing on its
own. One file per person means two people signing up at the same moment never touch the same
path — there is nothing to overwrite and nothing to lose — and removing somebody is removing
one file, which matters the first time you are asked to.

`country` comes from Cloudflare's `CF-IPCountry` header, so on Deno Deploy it is simply null.
If you would rather never keep it at all, delete the line in `receiver.js`.

Export is `git clone` and `cat enrollments/*.json`. When you do want a mailing tool later, the
list is already yours in a format anything will take.

## The counter is real

`GET` on the receiver returns the number of files in `enrollments/`. There is no floor and no
head start — the template this came from hardcoded `1,284 already waiting` and started
positions at 1284; both are gone. The line is hidden at zero, because "0 already waiting"
reads worse than saying nothing, and appears from the first enrollment onwards.

The count shown on the page is cached for 30 seconds so a busy launch day doesn't spend a
GitHub API request per visitor. **Positions are never cached** — a position is a claim about
where somebody stands, and a stale number hands the same one to two people.

## Spam

There is a honeypot field no person can see; anything that fills it gets a plausible success
and nothing is written. Beyond that the receiver checks the address, caps the wish at 200 words,
drops unknown platform values, and refuses posts from any origin but the page.

That is enough for a quiet launch and not enough for a targeted one. If junk starts arriving,
the next step is a challenge in front of the form — hCaptcha or Turnstile, both free and
about fifteen lines.

## Tests

```bash
cd worker && node receiver.test.mjs
```

Eleven checks against a stubbed GitHub — no network, no account, and no host. They run
`handle`, which is the whole receiver, so Cloudflare and Deno Deploy are covered by the same
pass: the first enrollment, the second, the same address twice, the honeypot, a bad address,
the word cap, a foreign origin, preflight, a missing token, and a GitHub outage. They found a real bug while
this was being written: the count cache was being used to assign positions as well as display
them, which would have given two people the same place.

## Where the design came from

`ColaiWaitlist.dc.html` is a Claude Design document. It cannot be served as a web page: it
needs the DC runtime, and it imports a hero component from a `colai-ad/` folder and fonts from
a `tokens/` folder that are not in this repository. `site/` is that design rebuilt as plain
HTML — same palette, type, grid, aurora and sweep — with a self-contained CSS animation of the
rail in place of the missing import. Edit the files in `site/`, not the `.dc.html`.
