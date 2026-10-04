//! What Claude Code is configured with — read for the settings panel's "Claude Code" tab.
//!
//! The toolbar already learns the live model, tools, slash commands, capabilities and permission mode
//! from the session's init frame (see `session.rs`); this module reads the parts that live on disk and
//! are not in that frame: installed/enabled plugins, skills, and MCP servers (connectors included).
//! Everything is read defensively — a missing or malformed file yields an empty list, never an error —
//! so the panel degrades to "none" rather than failing.
//!
//! One thing here writes: `colai_claude_toggle` enables or disables a plugin by editing the one small,
//! well-understood field (`enabledPlugins` in `~/.claude/settings.json`), as a careful read-modify-write
//! that preserves every other key. MCP and the rest stay read-only.

use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;

/// The user's `.claude` directory, or `None` if the home cannot be found.
fn claude_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".claude"))
}

/// The `~/.claude.json` file (sibling of the `.claude` dir), where per-project MCP config lives.
fn claude_json() -> Option<PathBuf> {
    claude_dir().and_then(|dir| dir.parent().map(|home| home.join(".claude.json")))
}

fn read_json(path: &PathBuf) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Everything the Claude Code tab shows that is not already in the session frame.
#[tauri::command]
pub(crate) async fn colai_claude_settings(cwd: Option<String>) -> Result<Value, String> {
    Ok(json!({
        "plugins": plugins(),
        "skills": skills(),
        "mcp": mcp_servers(cwd.as_deref()),
        "settings": small_settings(),
    }))
}

/// Installed plugins crossed with which are enabled.
fn plugins() -> Vec<Value> {
    let Some(dir) = claude_dir() else { return Vec::new() };
    let enabled: Map<String, Value> = read_json(&dir.join("settings.json"))
        .and_then(|v| v.get("enabledPlugins").cloned())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let installed = read_json(&dir.join("plugins").join("installed_plugins.json"))
        .and_then(|v| v.get("plugins").cloned())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (name, entries) in &installed {
        seen.insert(name.clone());
        let scope = entries
            .as_array()
            .and_then(|a| a.first())
            .and_then(|e| e.get("scope"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        out.push(json!({
            "name": name,
            "scope": scope,
            "enabled": enabled.get(name).and_then(Value::as_bool).unwrap_or(false),
        }));
    }
    // Enabled but not in the installed index (edge): still show it as on.
    for (name, on) in &enabled {
        if !seen.contains(name) && on.as_bool().unwrap_or(false) {
            out.push(json!({ "name": name, "scope": "", "enabled": true }));
        }
    }
    out.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    out
}

/// Skills available, by readable name, from the user and project skill directories.
fn skills() -> Vec<Value> {
    let mut out = Vec::new();
    let mut add_dir = |root: PathBuf, scope: &str| {
        let Ok(entries) = fs::read_dir(&root) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let base = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            // A `synced` folder holds skills under opaque ids; look one level in for each one's SKILL.md.
            if base == "synced" {
                if let Ok(inner) = fs::read_dir(&path) {
                    for sub in inner.flatten() {
                        if sub.path().is_dir() {
                            if let Some(name) = skill_name(&sub.path()) {
                                out.push(json!({ "name": name, "scope": scope }));
                            }
                        }
                    }
                }
                continue;
            }
            let name = skill_name(&path).unwrap_or(base);
            out.push(json!({ "name": name, "scope": scope }));
        }
    };
    if let Some(dir) = claude_dir() {
        add_dir(dir.join("skills"), "user");
    }
    if let Some(project) = std::env::current_dir().ok() {
        add_dir(project.join(".claude").join("skills"), "project");
    }
    out.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    out.dedup_by(|a, b| a["name"] == b["name"]);
    out
}

/// A skill's declared name (SKILL.md frontmatter `name:`), or `None` if there is no SKILL.md.
fn skill_name(dir: &std::path::Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    for line in text.lines().take(20) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let name = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    // No frontmatter name — fall back to the folder's own name unless it is an opaque id.
    dir.file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.contains('_') || n.len() < 40)
        .map(str::to_string)
}

/// MCP servers (connectors are the remote ones) — global and for the current project.
fn mcp_servers(cwd: Option<&str>) -> Vec<Value> {
    let Some(config) = claude_json().and_then(|p| read_json(&p)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut push = |servers: &Value, scope: &str, enabled_of: &dyn Fn(&str) -> bool| {
        if let Some(obj) = servers.as_object() {
            for (name, def) in obj {
                let transport = def
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| def.get("url").and(Some("remote".to_string())))
                    .unwrap_or_else(|| "stdio".to_string());
                out.push(json!({
                    "name": name,
                    "transport": transport,
                    "scope": scope,
                    "enabled": enabled_of(name),
                }));
            }
        }
    };
    if let Some(global) = config.get("mcpServers") {
        push(global, "user", &|_| true);
    }
    if let Some(cwd) = cwd {
        if let Some(project) = config.get("projects").and_then(|p| p.get(cwd)) {
            let enabled: Vec<String> = project
                .get("enabledMcpjsonServers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let disabled: Vec<String> = project
                .get("disabledMcpjsonServers")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            if let Some(servers) = project.get("mcpServers") {
                push(servers, "project", &|name| {
                    !disabled.iter().any(|d| d == name)
                        && (enabled.is_empty() || enabled.iter().any(|e| e == name))
                });
            }
        }
    }
    out.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    out
}

/// The small, safe-to-show bits of the global settings.
fn small_settings() -> Value {
    let settings = claude_dir()
        .and_then(|dir| read_json(&dir.join("settings.json")))
        .unwrap_or(Value::Null);
    json!({
        "theme": settings.get("theme"),
        "effortLevel": settings.get("effortLevel"),
        "autoUpdatesChannel": settings.get("autoUpdatesChannel"),
    })
}

/// Enable or disable a plugin, by editing `enabledPlugins` in `~/.claude/settings.json` and nothing else.
///
/// A read-modify-write that preserves every other key, written atomically (temp file + rename). The only
/// Claude Code config colai writes. Takes effect when Claude Code next reads its settings (a reload).
#[tauri::command]
pub(crate) async fn colai_claude_toggle(name: String, enable: bool) -> Result<(), String> {
    let dir = claude_dir().ok_or("could not find your .claude directory")?;
    let path = dir.join("settings.json");
    let mut root: Value = read_json(&path).unwrap_or_else(|| json!({}));
    let obj = root.as_object_mut().ok_or("settings.json is not an object")?;
    let plugins = obj
        .entry("enabledPlugins")
        .or_insert_with(|| Value::Object(Map::new()));
    let map = plugins
        .as_object_mut()
        .ok_or("enabledPlugins is not an object")?;
    if enable {
        map.insert(name, Value::Bool(true));
    } else {
        map.remove(&name);
    }
    let text = serde_json::to_string_pretty(&root).map_err(|e| format!("could not encode settings: {e}"))?;
    write_atomic(&path, &text)
}

/// Write `text` to `path` without a torn file: to a temp beside it, then rename over.
fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.colai-tmp");
    fs::write(&tmp, text).map_err(|e| format!("could not write settings: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("could not replace settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_toggle_flips_one_field_and_keeps_the_rest() {
        // The writer must change only enabledPlugins and preserve every other setting.
        let dir = std::env::temp_dir().join(format!("colai-claude-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"enabledPlugins":{"a@m":true},"theme":"light","effortLevel":"high"}"#,
        )
        .unwrap();

        // Reuse the write logic directly on this file by faking the home via the same shape.
        let mut root: Value = read_json(&path).unwrap();
        {
            let map = root
                .as_object_mut()
                .unwrap()
                .get_mut("enabledPlugins")
                .unwrap()
                .as_object_mut()
                .unwrap();
            map.insert("b@m".into(), Value::Bool(true)); // enable b
            map.remove("a@m"); // disable a
        }
        write_atomic(&path, &serde_json::to_string_pretty(&root).unwrap()).unwrap();

        let after: Value = read_json(&path).unwrap();
        assert_eq!(after["theme"], "light", "other keys are preserved");
        assert_eq!(after["effortLevel"], "high");
        assert_eq!(after["enabledPlugins"]["b@m"], true, "b is enabled");
        assert!(after["enabledPlugins"].get("a@m").is_none(), "a is disabled (removed)");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_source_reads_as_empty_not_an_error() {
        // The reader never throws on a missing file — it yields empty lists.
        assert!(read_json(&PathBuf::from("/no/such/colai-xyzzy.json")).is_none());
    }
}
