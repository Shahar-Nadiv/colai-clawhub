// A small, safe markdown -> DOM renderer for agent text.
//
// Agent replies arrive as markdown (headings, lists, **bold**, `code`, fenced blocks). Both the
// response card and the Work panel used to print them as raw textContent, so structure showed as
// literal `#`/`**`/``` characters and multi-paragraph answers collapsed into a run-on line. This
// renders that markdown the way Cursor does — readable and typeset — while staying inside the one
// safety rule the rest of the toolbar keeps: model text never reaches `innerHTML`. Every piece is
// built with `createElement` + `textContent`, so anything that is not recognised markdown (an
// `<script>` tag, a stray `<b>`) is inert text, not markup.
//
// A classic script sharing one global scope with the rest; `renderMarkdown(text)` returns a
// DocumentFragment the caller appends into a `.md` container.

/** The one entry point: markdown string -> a fragment of DOM nodes. */
function renderMarkdown(text) {
  const frag = document.createDocumentFragment();
  const lines = String(text == null ? "" : text).replace(/\r\n?/g, "\n").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // Blank line — paragraph separator, nothing to emit.
    if (line.trim() === "") {
      i++;
      continue;
    }

    // Fenced code block — parsed first, so `#`/`-`/`*` inside code are never treated as markdown.
    const fence = line.match(/^\s*```+\s*([\w+#.-]*)\s*$/);
    if (fence) {
      const body = [];
      i++;
      while (i < lines.length && !/^\s*```+\s*$/.test(lines[i])) {
        body.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // consume the closing fence
      frag.append(mdCodeBlock(body.join("\n"), fence[1] || ""));
      continue;
    }

    // Heading (one to three hashes).
    const heading = line.match(/^\s*(#{1,3})\s+(.*)$/);
    if (heading) {
      const el = document.createElement("div");
      el.className = heading[1].length === 1 ? "md-h1" : heading[1].length === 2 ? "md-h2" : "md-h3";
      mdInline(el, heading[2].trim());
      frag.append(el);
      i++;
      continue;
    }

    // Horizontal rule.
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      const hr = document.createElement("hr");
      hr.className = "md-hr";
      frag.append(hr);
      i++;
      continue;
    }

    // Blockquote — consecutive `>` lines.
    if (/^\s*>\s?/.test(line)) {
      const quoted = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) {
        quoted.push(lines[i].replace(/^\s*>\s?/, ""));
        i++;
      }
      const bq = document.createElement("blockquote");
      bq.className = "md-quote";
      mdInline(bq, quoted.join(" "));
      frag.append(bq);
      continue;
    }

    // List — consecutive items, one nesting level by indent.
    if (/^\s*([-*]|\d+[.)])\s+/.test(line)) {
      const built = mdList(lines, i);
      frag.append(built.list);
      i = built.next;
      continue;
    }

    // Paragraph — consecutive lines until a blank line or a structural line.
    const para = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^\s*```+/.test(lines[i]) &&
      !/^\s*#{1,3}\s+/.test(lines[i]) &&
      !/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i]) &&
      !/^\s*>\s?/.test(lines[i]) &&
      !/^\s*([-*]|\d+[.)])\s+/.test(lines[i])
    ) {
      para.push(lines[i]);
      i++;
    }
    const p = document.createElement("p");
    p.className = "md-p";
    mdInline(p, para.join(" "));
    frag.append(p);
  }
  return frag;
}

/** A fenced code block: a mono panel with a language label and a Copy button. */
function mdCodeBlock(body, lang) {
  const wrap = document.createElement("div");
  wrap.className = "md-codewrap";
  const head = document.createElement("div");
  head.className = "md-codehead";
  const tag = document.createElement("span");
  tag.className = "md-codelang";
  tag.textContent = lang || "code";
  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "md-copy";
  copy.textContent = "Copy";
  copy.addEventListener("click", () => mdCopy(copy, body));
  head.append(tag, copy);
  const pre = document.createElement("pre");
  pre.className = "md-code";
  const code = document.createElement("code");
  code.textContent = body;
  pre.append(code);
  wrap.append(head, pre);
  return wrap;
}

/**
 * Copy a code block's text, with a quiet confirmation on the button.
 *
 * `label` is what the button goes back to saying afterwards — "Copy" for a code block, and
 * whatever its own name is for a button elsewhere that copies one fixed thing (the Work
 * panel's "Copy /rewind").
 */
function mdCopy(button, text, label) {
  const settle = (word) => {
    button.textContent = word;
    setTimeout(() => {
      button.textContent = label || "Copy";
    }, 1200);
  };
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(
        () => settle("Copied"),
        () => settle("Couldn't copy"),
      );
      return;
    }
  } catch {
    /* fall through */
  }
  settle("Couldn't copy");
}

/** Build a list (ul/ol) from consecutive items; returns the node and the next line index. */
function mdList(lines, start) {
  const first = lines[start].match(/^(\s*)([-*]|\d+[.)])\s+/);
  const ordered = /\d/.test(first[2]);
  const baseIndent = first[1].length;
  const list = document.createElement(ordered ? "ol" : "ul");
  list.className = "md-list";
  let i = start;
  let currentItem = null;
  let sublist = null;
  while (i < lines.length) {
    if (lines[i].trim() === "") break;
    const item = lines[i].match(/^(\s*)([-*]|\d+[.)])\s+(.*)$/);
    if (!item) break;
    const indent = item[1].length;
    if (indent >= baseIndent + 2 && currentItem) {
      if (!sublist) {
        sublist = document.createElement(/\d/.test(item[2]) ? "ol" : "ul");
        sublist.className = "md-sublist";
        currentItem.appendChild(sublist);
      }
      const sub = document.createElement("li");
      mdInline(sub, item[3]);
      sublist.appendChild(sub);
    } else {
      const li = document.createElement("li");
      mdInline(li, item[3]);
      list.appendChild(li);
      currentItem = li;
      sublist = null;
    }
    i++;
  }
  return { list, next: i };
}

/** Append `str` to `parent` as text plus inline nodes (bold, italic, code, links). */
function mdInline(parent, str) {
  let rest = String(str).replace(/\n+/g, " ");
  let guard = 0;
  while (rest.length && guard++ < 5000) {
    const hit = mdNextSpan(rest);
    if (!hit) {
      parent.appendChild(document.createTextNode(rest));
      return;
    }
    if (hit.index > 0) parent.appendChild(document.createTextNode(rest.slice(0, hit.index)));
    parent.appendChild(hit.node);
    rest = rest.slice(hit.index + hit.length);
  }
  if (rest.length) parent.appendChild(document.createTextNode(rest));
}

/** The earliest inline span in `str`, or null. Code wins first so markup inside it stays literal. */
function mdNextSpan(str) {
  const patterns = [
    { re: /`([^`]+)`/, make: (m) => mdSpan("code", m[1]) },
    { re: /\*\*([^*]+)\*\*/, make: (m) => mdSpan("strong", m[1]) },
    { re: /__([^_]+)__/, make: (m) => mdSpan("strong", m[1]) },
    { re: /\*([^*\s][^*]*)\*/, make: (m) => mdSpan("em", m[1]) },
    { re: /\[([^\]]+)\]\(([^)\s]+)\)/, make: (m) => mdLink(m[1], m[2]) },
  ];
  let best = null;
  for (const pattern of patterns) {
    const m = pattern.re.exec(str);
    if (m && (best === null || m.index < best.index)) {
      best = { index: m.index, length: m[0].length, node: pattern.make(m) };
    }
  }
  return best;
}

/** One inline element whose content is plain text (no nesting, so nothing can smuggle markup in). */
function mdSpan(kind, text) {
  if (kind === "code") {
    const el = document.createElement("code");
    el.className = "md-ic";
    el.textContent = text;
    return el;
  }
  const el = document.createElement(kind);
  el.textContent = text;
  return el;
}

/** A link that opens externally on click; inert styled text if there is nothing to open with. */
function mdLink(label, url) {
  const a = document.createElement("a");
  a.className = "md-link";
  a.textContent = label;
  a.title = url;
  if (/^(https?:|mailto:)/i.test(url)) {
    a.addEventListener("click", (event) => {
      event.preventDefault();
      mdOpenExternal(url);
    });
  }
  return a;
}

/** Hand a URL to the OS browser through whatever opener this host exposes; quietly do nothing if none. */
function mdOpenExternal(url) {
  try {
    const tauri = window["__TAURI__"];
    if (tauri && tauri.opener && typeof tauri.opener.openUrl === "function") {
      void tauri.opener.openUrl(url);
      return;
    }
    if (typeof invoke === "function") {
      void invoke("plugin:opener|open_url", { url }).catch(() => {});
    }
  } catch {
    /* no opener — the label is still shown as styled text */
  }
}
