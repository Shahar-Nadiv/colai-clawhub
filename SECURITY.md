# Security

colai hands a picture and a sentence to an agent, and streams the agent's reply back onto
the screen. Both directions cross a trust boundary — what the agent does on your machine,
and what the agent's text is allowed to do inside the toolbar — so each is written down
here rather than left to be discovered.

## Reporting

Report it privately through GitHub: **[open a security advisory](https://github.com/Shahar-Nadiv/colai-clawhub/security/advisories/new)**
with what you found and how to reproduce it — or email **shacharnadiv@gmail.com**. Please do
not open a public issue for a vulnerability until there has been a chance to fix it.

## What the agent can do on your machine

- **No shell.** The `claude` process colai owns is spawned directly, with its arguments as
  an argument vector — never through `sh -c`. A sentence you send, or one an agent writes,
  is data to that process, not a command line for it to interpret.
- **Writes are confined to the working directory.** The agent runs in permission mode
  `acceptEdits`, which lets it edit files under the conversation's working directory without
  asking and falls back to the default (ask) mode for anything outside it. Opening the
  toolbar onto a conversation is opening it onto that conversation's directory.
- **Reads go through a confine gate.** Before colai reads a file to attach it to a mark, the
  path is `canonicalize`d — symlinks and `..` resolved to a real location — and then checked
  to be inside the root it is allowed to read from. Canonicalizing *first* is the point: a
  symlink that points out of the root resolves to its real target and is rejected, rather
  than being confined by its spelling. On top of that there is a `never_named` denylist
  (`colai_files.rs`) for files that are off limits whatever their path — `.env` and its
  kin — so a secret is refused even when it sits inside the allowed root.

## What the agent's text can do inside the toolbar

The toolbar is a webview, and an agent's reply is attacker-influenced text rendered in it.
Three layers keep that text from becoming code or a trap.

- **CSP.** `script-src 'self'` and `object-src 'none'` mean no inline script, no injected
  `<script>` and no plugins can run — only the toolbar's own bundled scripts. `freezePrototype`
  is on, so a reply cannot reach in and redefine a built-in the toolbar relies on.
- **Safe DOM.** Agent text reaches the DOM only as `textContent`, never as `innerHTML`. The
  markdown renderer (`toolbar-markdown.js`) builds every node with `createElement` and sets
  its text through `textContent`, so anything that is not recognised markdown — a stray
  `<script>`, an `<img onerror=…>` — shows as the characters it is and does nothing.
- **Links.** A markdown link an agent writes is kept only if its URL is `http(s):` or
  `mailto:` — a `javascript:` or `file:` URL is dropped. A kept link still never navigates on
  its own; it opens only when you click it, in your browser. This is the phishing surface: an
  agent can propose a link, so treat one in a reply as you would a link in an email from a
  stranger, and read where it goes before you click.

## The unattended scheduler

A scheduled task runs colai without you sitting in front of it. It runs with the same
`acceptEdits` confinement as an interactive turn — writes limited to the task's own working
directory — but there is no person to decline the one prompt a write outside it would raise.
Point a scheduled task at a directory you are willing to have edited without a second look,
and nowhere wider.
