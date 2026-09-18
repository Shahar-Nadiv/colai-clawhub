#!/usr/bin/env node
/*
 * What `claude plugin validate --strict` does not check.
 *
 * The validator reads the manifests: JSON syntax, required fields, component paths,
 * unrecognised keys. It is good at that and you should run it. It caught exactly one of the
 * faults below — a `README.md` in `commands/` — and none of the rest, because the rest are
 * not manifest problems. They are things that are perfectly valid on disk and wrong at
 * runtime, which is the worst combination: the plugin installs, loads, reports no error, and
 * then does not work.
 *
 * Each check below shipped, in a real plugin, to real users. The message names the symptom
 * rather than the rule, because the symptom is what somebody arrives with — "my command does
 * nothing and says nothing" is how you find the `$` rule, not the other way round.
 *
 *   node lint-plugin.mjs [path-to-plugin]        one plugin
 *   node lint-plugin.mjs --all [dir-of-plugins]  every plugin under a directory
 *
 * Exits non-zero if anything is a problem rather than a note, so it can gate a release.
 *
 * Verified against Claude Code 2.1.274. Checks that depend on a version say so.
 */
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join, basename, relative } from "node:path";
import { execFileSync } from "node:child_process";

/* ── what the runtime actually accepts ──────────────────────────────────────── */

/**
 * Every hook event, from the documentation and confirmed against the binary.
 *
 * Worth stating how this list was arrived at, because getting it wrong is easy and I did:
 * a first pass grepped the binary for event names anchored to line starts and found eleven,
 * which looked like a complete answer and was a third of one. Thirty of these thirty-three
 * appear verbatim in the 2.1.274 binary; the three that do not (`Stop`, `Setup`,
 * `SessionStart`) are short enough to live inside other strings.
 *
 * A misspelled event in `hooks.json` does not error. It simply never fires.
 */
const HOOK_EVENTS = new Set([
  "SessionStart", "Setup", "UserPromptSubmit", "UserPromptExpansion", "PreToolUse",
  "PermissionRequest", "PermissionDenied", "PostToolUse", "PostToolUseFailure",
  "PostToolBatch", "Notification", "MessageDisplay", "SubagentStart", "SubagentStop",
  "TaskCreated", "TaskCompleted", "Stop", "StopFailure", "TeammateIdle",
  "InstructionsLoaded", "ConfigChange", "CwdChanged", "DirectoryAdded", "FileChanged",
  "WorktreeCreate", "WorktreeRemove", "PreCompact", "PostCompact", "PreModelSwitch",
  "PostModelSwitch", "Elicitation", "ElicitationResult", "SessionEnd",
]);

/**
 * Frontmatter a skill or command may carry.
 *
 * An unrecognised key is discarded silently at load, which is why this check exists: a
 * mistyped `allowed_tools` (underscore, not hyphen) reads to the author as a setting that is
 * set, and to Claude Code as nothing at all. That failure is invisible in every direction
 * except behaviour.
 */
const SKILL_FIELDS = new Set([
  "name", "description", "when_to_use", "argument-hint", "arguments",
  "disable-model-invocation", "user-invocable", "allowed-tools", "disallowed-tools",
  "model", "effort", "context", "agent", "background", "hooks", "paths", "shell",
  "metadata", "license", "compatibility",
]);

/** `description` and `when_to_use` share this budget in the skill listing. Documented. */
const LISTING_CAP = 1536;

/**
 * Lockfiles the installer will act on, and the ones it ignores.
 *
 * A plugin is installed by copying its checkout. If one of these is in it, Claude Code runs
 * a frozen dependency install over the manifest beside it — always with `--ignore-scripts`,
 * so it is not an execution risk, but it is a download. In the case this check comes from,
 * the manifest beside it belonged to an unrelated npm wrapper and a 14 MB clone became a
 * 603 MB install for people who wanted a shell script and a binary.
 */
const LOCKFILES_THAT_INSTALL = ["package-lock.json", "bun.lock", "npm-shrinkwrap.json"];
const LOCKFILES_IGNORED = ["yarn.lock", "pnpm-lock.yaml"];

/* ── findings ───────────────────────────────────────────────────────────────── */

const found = [];
/*
 * Said once. `security-guidance` has nine `PostToolUse` hooks with no timeout, and nine
 * identical paragraphs about it is not nine times as useful as one.
 */
const already = new Set();
const remember = (level, where, said) => {
  const key = `${level}\u0000${where}\u0000${said}`;
  if (already.has(key)) return;
  already.add(key);
  found.push({ level, where, said });
};
const note = (where, said) => remember("note", where, said);
const problem = (where, said) => remember("problem", where, said);

const read = (path) => {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
};
const jsonAt = (path) => {
  const said = read(path);
  if (said === null) return null;
  try {
    return JSON.parse(said);
  } catch (trouble) {
    problem(path, `is not valid JSON (${trouble.message}) — the plugin will not load`);
    return null;
  }
};
const filesIn = (dir, ext) => {
  try {
    return readdirSync(dir).filter((name) => name.endsWith(ext));
  } catch {
    return [];
  }
};

/** The frontmatter block and the body, or null when there is no frontmatter. */
function frontmatter(text) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(text);
  if (!match) return null;
  const fields = {};
  let key = null;
  for (const line of match[1].split(/\r?\n/)) {
    const pair = /^([A-Za-z_][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (pair) {
      key = pair[1];
      fields[key] = pair[2];
    } else if (key && /^\s+\S/.test(line)) {
      // A continuation or list item belonging to the previous key.
      fields[key] += " " + line.trim();
    }
  }
  return { fields, body: match[2] };
}

/* ── the checks ─────────────────────────────────────────────────────────────── */

/**
 * A `!` line is permission-checked before it runs, and refused if it contains a `$`.
 *
 * This is the one that costs the most time, because the failure is silent in the place you
 * are looking. `/x:show` does nothing, says nothing, and the reason —
 * `Shell command permission check failed for pattern …: Contains simple_expansion` — appears
 * only in the session transcript. Everything that needs an environment has to read it
 * further down, in a script the `!` line calls.
 */
function checkBangLines(where, text) {
  const lines = text.split(/\r?\n/);
  /*
   * Fenced blocks are skipped, and finding that out is why this lint was run against
   * Anthropic's own plugins before being trusted. `command-development/SKILL.md` documents
   * the `!` syntax, so it is full of examples of it — including deliberately bad ones. Nine
   * false alarms in one file, all of them prose about the very rule being checked.
   */
  let fenced = false;
  lines.forEach((line, at) => {
    if (/^\s*(```|~~~)/.test(line)) {
      fenced = !fenced;
      return;
    }
    if (fenced) return;
    const bang = /(^|\s)!`([^`]*)`/.exec(line);
    if (!bang) return;
    const ran = bang[2];
    if (ran.includes("$")) {
      problem(
        `${where}:${at + 1}`,
        "a `!` line contains `$`. The permission check refuses it as `simple_expansion` " +
          "before it runs, so the command does nothing AND says nothing — the reason only " +
          "appears in the transcript. Move anything needing the environment into a script " +
          "the line calls.",
      );
    }
    for (const trick of ["setsid", "nohup"]) {
      if (ran.includes(trick)) {
        problem(
          `${where}:${at + 1}`,
          `a \`!\` line uses \`${trick}\`. The sandbox refuses it as something it cannot ` +
            "statically analyse; a long-running program has to put itself in its own " +
            "process group instead.",
        );
      }
    }
    if (/(^|[^&])&\s*$/.test(ran)) {
      problem(
        `${where}:${at + 1}`,
        "a `!` line backgrounds with `&`. Detaching belongs in the program, not the " +
          "command — and backgrounding a program that also backgrounds itself loses its " +
          "exit status.",
      );
    }
  });
}

/** Commands, and the documentation that accidentally becomes one. */
function checkCommands(root) {
  const dir = join(root, "commands");
  if (!existsSync(dir)) return;
  note(
    "commands/",
    "`commands/` is the legacy layout. New plugins should put user-invoked commands in " +
      "`skills/<name>/SKILL.md` — both load identically. See plugin-dev's " +
      "command-development skill, which says so itself.",
  );
  for (const name of filesIn(dir, ".md")) {
    const stem = basename(name, ".md");
    if (/^(readme|contributing|changelog|license|notes?|todo|docs?)$/i.test(stem)) {
      problem(
        `commands/${name}`,
        `every .md in commands/ becomes a listable command, so this registers as ` +
          `\`/<plugin>:${stem}\` for everybody who installs the plugin. Move it to docs/.`,
      );
    }
    const text = read(join(dir, name));
    if (text) {
      checkBangLines(`commands/${name}`, text);
      if (/\bRun exactly this\b|\brun exactly this\b/.test(text) && !/(^|\s)!`/.test(text)) {
        note(
          `commands/${name}`,
          "the body asks Claude to run something instead of running it. A `!` line " +
            "executes before the model sees anything; a prompt saying 'run this' leaves " +
            "the model free to decide otherwise.",
        );
      }
    }
  }
}

/** Skills: the listing budget, the unknown key, and the description that says only what. */
function checkSkills(root) {
  const dirs = [];
  const skillsDir = join(root, "skills");
  if (existsSync(skillsDir)) {
    for (const name of readdirSync(skillsDir)) {
      const at = join(skillsDir, name, "SKILL.md");
      if (existsSync(at)) dirs.push([`skills/${name}/SKILL.md`, at]);
    }
  }
  // A plugin with one skill may put SKILL.md at its root.
  if (existsSync(join(root, "SKILL.md"))) dirs.push(["SKILL.md", join(root, "SKILL.md")]);

  for (const [where, at] of dirs) {
    const text = read(at);
    if (!text) continue;
    const parsed = frontmatter(text);
    if (!parsed) {
      problem(where, "has no YAML frontmatter, so it carries no description and cannot be triggered by the model.");
      continue;
    }
    const { fields, body } = parsed;

    /*
     * Only near-misses, and that is a correction.
     *
     * This first flagged every key not on the documented list, and then flagged `version`
     * in a dozen of Anthropic's own skills and `tools` in two more. Either those are real
     * fields the documentation does not name, or Anthropic ships dead keys in its own
     * plugins; on that evidence the honest conclusion is that my list is incomplete and the
     * frontmatter is extensible by design — the Agent Skills spec has fields Claude Code
     * merely tolerates, and `metadata` exists precisely so authors can add their own.
     *
     * So an unrecognised key is not a finding. What is a finding is a key that differs from
     * a real one only in punctuation or case: `allowed_tools` for `allowed-tools` is a
     * setting the author believes is set, discarded silently at load, and invisible in every
     * direction except behaviour. That is the failure worth catching, and it is the only one
     * this can be sure about.
     */
    const flatten = (key) => key.toLowerCase().replace(/[-_\s]/g, "");
    const realByShape = new Map([...SKILL_FIELDS].map((key) => [flatten(key), key]));
    for (const key of Object.keys(fields)) {
      if (SKILL_FIELDS.has(key)) continue;
      const meant = realByShape.get(flatten(key));
      if (meant) {
        problem(
          where,
          `frontmatter key \`${key}\` differs from \`${meant}\` only in punctuation. ` +
            "Claude Code discards keys it does not recognise without complaining, so this " +
            "reads as a setting that is set and is not.",
        );
      }
    }

    const description = fields.description ?? "";
    if (!description) {
      problem(where, "has no `description`. That text is the whole basis on which Claude decides to use the skill.");
    } else {
      const budget = description.length + (fields.when_to_use ?? "").length;
      if (budget > LISTING_CAP) {
        problem(
          where,
          `\`description\` plus \`when_to_use\` is ${budget} characters and the skill ` +
            `listing truncates at ${LISTING_CAP} — the end of it never reaches the model.`,
        );
      }
      if (!/\b(use|when|trigger|asks?|wants?|mentions?)\b/i.test(description)) {
        note(
          where,
          "the `description` says what the skill does but never when to use it. That text " +
            "is the only thing Claude sees when deciding, and skills are under-triggered " +
            "rather than over-triggered, so name the situations out loud.",
        );
      }
    }

    checkBangLines(where, text);

    const words = body.trim().split(/\s+/).length;
    const hasMore =
      existsSync(join(at, "..", "references")) || existsSync(join(at, "..", "scripts"));
    if (words > 3000 && !hasMore) {
      note(
        where,
        `the body is about ${words} words with no references/ or scripts/ beside it. The ` +
          "body is paid for on every use; detail that is only sometimes needed belongs in " +
          "a file the skill points at.",
      );
    }
  }
}

/** Hooks: the event that never fires, and the hook that hangs somebody's prompt. */
function checkHooks(root) {
  const at = join(root, "hooks", "hooks.json");
  if (!existsSync(at)) return;
  const wiring = jsonAt(at);
  if (!wiring) return;
  const events = wiring.hooks ?? {};
  for (const [event, groups] of Object.entries(events)) {
    if (!HOOK_EVENTS.has(event)) {
      problem(
        "hooks/hooks.json",
        `\`${event}\` is not a hook event. A misspelled event does not error — it simply ` +
          "never fires, so the hook looks installed and never runs.",
      );
    }
    for (const group of Array.isArray(groups) ? groups : []) {
      for (const one of group.hooks ?? []) {
        if (one.type === "command" && !(one.timeout > 0)) {
          note(
            "hooks/hooks.json",
            `the ${event} hook sets no \`timeout\`. It runs in front of somebody's prompt; ` +
              "a command that hangs hangs them.",
          );
        }
        const named = /([\w./$}{-]+\.(sh|mjs|js|py))/.exec(one.command ?? "");
        if (!named) continue;
        const script = join(root, named[1].replace(/^.*CLAUDE_PLUGIN_ROOT}?\/?/, ""));
        if (!existsSync(script)) continue;
        const text = read(script) ?? "";
        if (!text.startsWith("#!")) {
          problem(relative(root, script), "is run as a hook but has no shebang.");
        }
        try {
          if (!(statSync(script).mode & 0o111)) {
            problem(relative(root, script), "is run as a hook but is not executable.");
          }
        } catch {}
      }
    }
  }
}

/** The manifests, and the two versions that have to agree. */
function checkManifests(root) {
  const plugin = jsonAt(join(root, ".claude-plugin", "plugin.json"));
  const market = jsonAt(join(root, ".claude-plugin", "marketplace.json"));
  if (plugin && !plugin.name) {
    problem(".claude-plugin/plugin.json", "has no `name`, which is what namespaces every command and skill in it.");
  }
  if (plugin && market) {
    const mine = (market.plugins ?? []).find((one) => one.name === plugin.name);
    if (mine && mine.version && plugin.version && mine.version !== plugin.version) {
      problem(
        ".claude-plugin/marketplace.json",
        `offers ${plugin.name} at ${mine.version} while plugin.json claims ` +
          `${plugin.version}. An install resolves a version that does not exist.`,
      );
    }
  }
  if (existsSync(join(root, ".claude-plugin", "output-styles"))) {
    note(
      ".claude-plugin/",
      "components live at the plugin root, not inside .claude-plugin/ — only plugin.json " +
        "and marketplace.json belong in there.",
    );
  }
  if (plugin?.outputStyles && existsSync(join(root, "outputStyles"))) {
    note(
      "outputStyles/",
      "the directory is `output-styles/` (hyphenated) while the manifest key is " +
        "`outputStyles` (camelCase). A camelCase directory is not found.",
    );
  }
  for (const key of ["commands", "agents", "workflows", "outputStyles"]) {
    if (typeof plugin?.[key] === "string" || Array.isArray(plugin?.[key])) {
      note(
        ".claude-plugin/plugin.json",
        `\`${key}\` REPLACES the default directory rather than adding to it — only ` +
          "`skills` adds. Anything in the default location stops loading.",
      );
    }
  }
}

/**
 * Whether git would carry this file into a clone.
 *
 * Presence on disk is not the question, and asking it that way produced a false positive on
 * the very plugin this check came from: it keeps a `package-lock.json` for local development
 * and gitignores it, so the file is there and an install never sees it. An install clones, so
 * what matters is what git would hand over.
 *
 * A directory that is not a repository is treated as travelling — a plugin distributed some
 * other way (an archive, a copied folder) really does carry everything in it.
 */
function travels(root, name) {
  const git = (args) => {
    try {
      execFileSync("git", ["-C", root, ...args], { stdio: ["ignore", "pipe", "ignore"] });
      return true;
    } catch {
      return false;
    }
  };
  if (!git(["rev-parse", "--is-inside-work-tree"])) return true;
  if (git(["ls-files", "--error-unmatch", name])) return true;
  return !git(["check-ignore", name]);
}

/** What travels into an install. */
function checkWhatShips(root) {
  for (const name of LOCKFILES_THAT_INSTALL) {
    if (existsSync(join(root, name)) && travels(root, name)) {
      problem(
        name,
        "a plugin is installed by copying its checkout, and this makes Claude Code run a " +
          "dependency install over the manifest beside it. In the case this check comes " +
          "from, a 14 MB clone became a 603 MB install. (`--ignore-scripts` is always set, " +
          "so it is a download rather than an execution risk. `yarn.lock` and " +
          "`pnpm-lock.yaml` are skipped by the installer.)",
      );
    }
  }
  for (const name of LOCKFILES_IGNORED) {
    if (existsSync(join(root, name))) {
      note(name, "is present but the installer skips this kind, so nothing is fetched at install time.");
    }
  }
  const bin = join(root, "bin");
  if (existsSync(bin)) {
    note(
      "bin/",
      "is on PATH inside a session only, and is not permitted at all in plugins " +
        "distributed through claude.ai organisation settings.",
    );
    for (const name of readdirSync(bin)) {
      try {
        if (statSync(join(bin, name)).isFile() && !(statSync(join(bin, name)).mode & 0o111)) {
          problem(`bin/${name}`, "is on PATH but is not executable.");
        }
      } catch {}
    }
  }
}

/* ── running it ─────────────────────────────────────────────────────────────── */

function lint(root) {
  found.length = 0;
  already.clear();
  const isPlugin =
    existsSync(join(root, ".claude-plugin")) ||
    ["commands", "skills", "agents", "hooks", "themes", "output-styles", "monitors", "workflows"]
      .some((dir) => existsSync(join(root, dir))) ||
    ["SKILL.md", ".mcp.json", ".lsp.json"].some((file) => existsSync(join(root, file)));
  if (!isPlugin) {
    return { skipped: true, found: [] };
  }
  checkManifests(root);
  checkCommands(root);
  checkSkills(root);
  checkHooks(root);
  checkWhatShips(root);
  return { skipped: false, found: [...found] };
}

function say(name, result) {
  if (result.skipped) return 0;
  const problems = result.found.filter((one) => one.level === "problem");
  const notes = result.found.filter((one) => one.level === "note");
  if (result.found.length === 0) {
    console.log(`\x1b[32m✓\x1b[0m ${name}`);
    return 0;
  }
  console.log(`\n\x1b[1m${name}\x1b[0m`);
  for (const one of problems) console.log(`  \x1b[31m✗\x1b[0m ${one.where}\n     ${one.said}`);
  for (const one of notes) console.log(`  \x1b[33m·\x1b[0m ${one.where}\n     ${one.said}`);
  return problems.length;
}

const args = process.argv.slice(2);
let wrong = 0;
if (args[0] === "--all") {
  const dir = args[1] ?? ".";
  for (const name of readdirSync(dir).sort()) {
    const at = join(dir, name);
    try {
      if (statSync(at).isDirectory()) wrong += say(name, lint(at));
    } catch {}
  }
} else {
  const at = args[0] ?? ".";
  wrong += say(relative(process.cwd(), at) || ".", lint(at));
}
if (wrong > 0) {
  console.log(`\n${wrong} problem(s). Notes are worth reading; problems will bite somebody.`);
}
process.exit(wrong > 0 ? 1 : 0);
