//! Virtual machines for agents: a machine of their own to build, test and deploy in, and a
//! window to watch it in — on this computer, like VirtualBox, with nothing for anyone to install.
//!
//! Colai brings its own VM engine. The first time somebody opens the VM key, it fetches QEMU's
//! official Windows build and unpacks it into its own folder — not through QEMU's installer
//! (which can ask for admin), but with 7-Zip's official standalone extractor — every file checked
//! against a hash pinned here, so nothing but those exact files is ever run. Then it builds the
//! base machine: Ubuntu's cloud image (checked against Ubuntu's sums), booted once to install an
//! XFCE desktop and developer tools. Every VM after that is a copy-on-write overlay
//! on it, made in seconds.
//!
//! QEMU uses Windows' hypervisor (WHPX) when the machine has it on, and plain emulation when it
//! does not — slower, never a prompt. The screen is VNC over a websocket on 127.0.0.1, shown in
//! the VM window by noVNC; QMP, also on 127.0.0.1, is how the toolbar stops it and takes its
//! thumbnail.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The Ubuntu release the base is built from: the current LTS cloud image.
const IMAGE_DIR: &str = "https://cloud-images.ubuntu.com/noble/current";
/// How big a machine's disk may grow. Overlays are sparse; this is a ceiling.
const DISK_SIZE: &str = "24G";

/// The engine, pinned: exactly these files, by hash, from their publishers.
struct Pinned {
    urls: &'static [&'static str],
    file: &'static str,
    sha256: Option<&'static str>,
    sha512: Option<&'static str>,
}

const SEVEN_ZR: Pinned = Pinned {
    urls: &["https://www.7-zip.org/a/7zr.exe"],
    file: "7zr.exe",
    sha256: Some("256feca8e274e5da655e2a284fabafd9f554365eb164862089dacd4e8276d282"),
    sha512: None,
};
const SEVEN_ZIP: Pinned = Pinned {
    urls: &["https://www.7-zip.org/a/7z2409-x64.exe"],
    file: "7z2409-x64.exe",
    sha256: Some("bdd1a33de78618d16ee4ce148b849932c05d0015491c34887846d431d29f308e"),
    sha512: None,
};
const QEMU_WINDOWS: Pinned = Pinned {
    urls: &[
        "https://qemu.weilnetz.de/w64/2026/qemu-w64-setup-20260811.exe",
        "https://qemu.weilnetz.de/w64/qemu-w64-setup-20260811.exe",
    ],
    file: "qemu-w64-setup-20260811.exe",
    sha256: None,
    sha512: Some(
        "5bcf9eed634e8575a37b74f445af41a2fe4106da512d0c30c368301d4c105037fdfab40a5287367a28a957624cddebbc8c07e16c88ab6634f554cdf3d16bf543",
    ),
};

/// The machine sizes the New VM form offers, by name.
pub(crate) fn size_of(name: &str) -> Option<(u32, u32)> {
    match name {
        "small" => Some((2, 2048)),
        "medium" => Some((2, 4096)),
        "large" => Some((4, 8192)),
        _ => None,
    }
}

/* ── where things live ─────────────────────────────────────────────────────────────────── */

/// `~/.claude/colai/vms`, created if missing.
fn vms_dir() -> Option<PathBuf> {
    let dir = crate::colai_schedule::colai_dir()?.join("vms");
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

/// The systems a VM can be: Ubuntu (the default) or the Windows Server evaluation.
pub(crate) const UBUNTU: &str = "ubuntu";
pub(crate) const WINDOWS: &str = "windows";

fn os_of(said: &str) -> Option<&'static str> {
    match said {
        UBUNTU => Some(UBUNTU),
        WINDOWS => Some(WINDOWS),
        _ => None,
    }
}

/// Each system's base machine, in a folder of its own.
fn base_dir_for(os: &str) -> Option<PathBuf> {
    let dir = vms_dir()?.join(if os == WINDOWS { "base-windows" } else { "base" });
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

fn base_dir() -> Option<PathBuf> {
    base_dir_for(UBUNTU)
}

/// Colai's own QEMU: the engine it brings, kept apart from anything else on the machine.
fn engine_dir() -> Option<PathBuf> {
    vms_dir().map(|dir| dir.join("engine"))
}

fn vm_dir(id: &str) -> Option<PathBuf> {
    // Ids are made here (`new_id`), but a command hands one back from the page: never a path.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(vms_dir()?.join(id))
}

/* ── the record of one VM ──────────────────────────────────────────────────────────────── */

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Ports {
    /// VNC display number; the raw VNC port is 5900 + this.
    pub vnc_display: u16,
    /// The websocket the VM window's noVNC connects to.
    pub ws: u16,
    pub qmp: u16,
    pub ssh: u16,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Vm {
    pub id: String,
    pub name: String,
    pub size: String,
    pub cpus: u32,
    pub memory_mb: u32,
    /// The conversation this machine belongs to. Only it may send work here.
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub owner_title: Option<String>,
    pub created_at: u64,
    /// Set while it runs; cleared when it stops.
    #[serde(default)]
    pub ports: Option<Ports>,
    #[serde(default)]
    pub pid: Option<u32>,
    /// When its one save point was made, if it has one.
    #[serde(default)]
    pub saved_at: Option<u64>,
    /// "ubuntu" or "windows". Machines made before there was a choice are Ubuntu.
    #[serde(default = "ubuntu")]
    pub os: String,
}

fn ubuntu() -> String {
    UBUNTU.to_string()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn new_id(now: u64) -> String {
    format!("vm-{:x}", now)
}

fn read_vm(id: &str) -> Option<Vm> {
    let text = fs::read_to_string(vm_dir(id)?.join("vm.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Written beside, then renamed over: a crash mid-write never leaves half a record.
fn write_vm(vm: &Vm) -> Result<(), String> {
    let dir = vm_dir(&vm.id).ok_or("not a VM id")?;
    fs::create_dir_all(&dir).map_err(|e| format!("could not make the VM folder: {e}"))?;
    let text = serde_json::to_string_pretty(vm).map_err(|e| format!("could not encode the VM: {e}"))?;
    let tmp = dir.join("vm.json.colai-tmp");
    fs::write(&tmp, text).map_err(|e| format!("could not write the VM: {e}"))?;
    fs::rename(&tmp, dir.join("vm.json")).map_err(|e| format!("could not save the VM: {e}"))
}

fn all_vms() -> Vec<Vm> {
    let Some(dir) = vms_dir() else { return Vec::new() };
    let mut vms: Vec<Vm> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("vm-"))
        .filter_map(|id| read_vm(&id))
        .collect();
    vms.sort_by_key(|vm| std::cmp::Reverse(vm.created_at));
    vms
}

/* ── the engine ────────────────────────────────────────────────────────────────────────── */

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Where the Linux engine's unpacked packages live: a root of their own, `usr/bin` and all.
fn engine_root() -> Option<PathBuf> {
    engine_dir().map(|dir| dir.join("root"))
}

/// A QEMU tool: Colai's own first, then (macOS, Linux) one the system already has.
fn find_tool(name: &str) -> Option<PathBuf> {
    let file = exe(name);
    let mut dirs: Vec<PathBuf> = engine_dir().into_iter().collect();
    dirs.extend(engine_root().map(|root| root.join("usr").join("bin")));
    if !cfg!(windows) {
        dirs.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
        dirs.push(PathBuf::from("/opt/homebrew/bin"));
        dirs.push(PathBuf::from("/usr/local/bin"));
        dirs.push(PathBuf::from("/usr/bin"));
    }
    dirs.into_iter().map(|dir| dir.join(&file)).find(|path| path.is_file())
}

/// A command for one of the engine's tools, told where its own libraries and modules are when
/// they are Colai's (Linux: unpacked packages, not installed ones).
fn engine_command(tool: &Path) -> Command {
    let mut cmd = Command::new(tool);
    if cfg!(target_os = "linux") {
        if let Some(root) = engine_root().filter(|root| tool.starts_with(root)) {
            let libs = [root.join("usr/lib/x86_64-linux-gnu"), root.join("lib/x86_64-linux-gnu")];
            let mut paths: Vec<PathBuf> = libs.to_vec();
            paths.extend(std::env::var_os("LD_LIBRARY_PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
            if let Ok(joined) = std::env::join_paths(paths) {
                cmd.env("LD_LIBRARY_PATH", joined);
            }
            cmd.env("QEMU_MODULE_DIR", root.join("usr/lib/x86_64-linux-gnu/qemu"));
        }
    }
    cmd
}

/// The firmware folders of Colai's own engine (`-L`), when it is Colai's own.
fn firmware_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = engine_dir().map(|dir| dir.join("share")).into_iter().collect();
    if let Some(root) = engine_root() {
        dirs.extend(["usr/share/qemu", "usr/share/seabios", "usr/lib/ipxe/qemu"].iter().map(|sub| root.join(sub)));
    }
    dirs.into_iter().filter(|dir| dir.is_dir()).collect()
}

fn system_binary() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "qemu-system-aarch64"
    } else {
        "qemu-system-x86_64"
    }
}

fn engine_ready() -> bool {
    find_tool(system_binary()).is_some() && find_tool("qemu-img").is_some()
}

/* ── building: the engine, then the base machine ───────────────────────────────────────── */

#[derive(Clone, Debug, PartialEq)]
enum Build {
    Idle,
    Fetching { what: &'static str, got: u64, of: u64 },
    Unpacking,
    Installing,
    Failed(String),
}

/// How far each system's build has got, by system.
fn build_states() -> &'static Mutex<std::collections::HashMap<&'static str, Build>> {
    static STATE: OnceLock<Mutex<std::collections::HashMap<&'static str, Build>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

thread_local! {
    /// Which system the build running on this thread is for, so `set_build` needs no argument.
    static BUILDING: std::cell::Cell<&'static str> = const { std::cell::Cell::new(UBUNTU) };
}

fn build_state_of(os: &str) -> Build {
    build_states()
        .lock()
        .ok()
        .and_then(|held| held.get(os).cloned())
        .unwrap_or(Build::Idle)
}

fn set_build(next: Build) {
    let os = BUILDING.with(|building| building.get());
    if let Ok(mut held) = build_states().lock() {
        held.insert(os, next);
    }
}

fn base_image_for(os: &str) -> Option<PathBuf> {
    base_dir_for(os).map(|dir| dir.join("base.qcow2"))
}

fn base_ready_for(os: &str) -> bool {
    base_dir_for(os).map(|dir| dir.join("ready").is_file()).unwrap_or(false)
}

fn image_name() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "noble-server-cloudimg-arm64.img"
    } else {
        "noble-server-cloudimg-amd64.img"
    }
}

fn run(cmd: &mut Command) -> Result<String, String> {
    crate::session::no_console_window(cmd);
    let out = cmd.output().map_err(|e| format!("could not run {:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// A hash, by the tool each OS ships with. `bits` is 256 or 512.
fn hash_of(path: &Path, bits: u32) -> Result<String, String> {
    let said = if cfg!(windows) {
        run(Command::new("certutil").arg("-hashfile").arg(path).arg(format!("SHA{bits}")))?
    } else if cfg!(target_os = "macos") {
        run(Command::new("shasum").args(["-a", &bits.to_string()]).arg(path))?
    } else {
        run(Command::new(format!("sha{bits}sum")).arg(path))?
    };
    let len = (bits / 4) as usize;
    said.split_whitespace()
        .find(|word| word.len() == len && word.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|hash| hash.to_ascii_lowercase())
        .ok_or_else(|| "could not read a checksum".to_string())
}

/// Ubuntu's published sum for one file, from its SHA256SUMS listing.
fn published_sum(listing: &str, file: &str) -> Option<String> {
    listing.lines().find_map(|line| {
        let (hash, name) = line.split_once(' ')?;
        (name.trim_start_matches([' ', '*']) == file).then(|| hash.to_ascii_lowercase())
    })
}

fn content_length(url: &str) -> u64 {
    run(Command::new("curl").args(["-sIL", url]))
        .ok()
        .and_then(|head| {
            head.lines()
                .filter_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|n| n.trim().to_string()))
                .filter_map(|n| n.parse::<u64>().ok())
                .last()
        })
        .unwrap_or(0)
}

/// Fetch `url` to `to`, saying how far it has got.
fn download(url: &str, to: &Path, what: &'static str) -> Result<(), String> {
    let of = content_length(url);
    let part = to.with_extension("part");
    let mut fetch = Command::new("curl");
    fetch.args(["-fL", "--retry", "3", "-o"]).arg(&part).arg(url);
    crate::session::no_console_window(&mut fetch);
    let mut child = fetch
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not download: {e}"))?;
    loop {
        set_build(Build::Fetching { what, got: fs::metadata(&part).map(|m| m.len()).unwrap_or(0), of });
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                let _ = fs::remove_file(&part);
                return Err(format!("could not download {what}"));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    fs::rename(&part, to).map_err(|e| e.to_string())
}

/// One pinned file, fetched and checked. A file that does not match is thrown away, never run.
fn fetch_pinned(pin: &Pinned, into: &Path, what: &'static str) -> Result<PathBuf, String> {
    let path = into.join(pin.file);
    let matches = |path: &Path| -> bool {
        match (pin.sha256, pin.sha512) {
            (Some(want), _) => hash_of(path, 256).map(|got| got == want).unwrap_or(false),
            (_, Some(want)) => hash_of(path, 512).map(|got| got == want).unwrap_or(false),
            _ => false,
        }
    };
    if path.is_file() && matches(&path) {
        return Ok(path);
    }
    let mut last = String::new();
    for url in pin.urls {
        match download(url, &path, what) {
            Ok(()) if matches(&path) => return Ok(path),
            Ok(()) => {
                let _ = fs::remove_file(&path);
                last = format!("{what} did not match its checksum, so it was thrown away");
            }
            Err(why) => last = why,
        }
    }
    Err(last)
}

/// Firmware the x86 machine needs; everything else QEMU ships for other systems is left out.
pub(crate) fn firmware_kept(name: &str) -> bool {
    name.starts_with("bios")
        || name.starts_with("vgabios")
        || name.starts_with("linuxboot")
        || name.starts_with("multiboot")
        || name.starts_with("edk2-x86_64")
        || name.starts_with("edk2-i386-vars")
        || matches!(name, "kvmvapic.bin" | "pvh.bin" | "efi-virtio.rom" | "keymaps")
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// Fetch QEMU into Colai's own folder. No installer, no admin, no password, no prompt.
fn get_engine() -> Result<(), String> {
    if cfg!(windows) {
        get_engine_windows()
    } else if cfg!(target_os = "linux") {
        get_engine_linux()
    } else {
        Err("VMs on macOS are coming next.".to_string())
    }
}

/// Packages to fetch: what QEMU needs, less what this system already has installed.
pub(crate) fn missing_packages(depends: &str, installed: impl Fn(&str) -> bool) -> Vec<String> {
    let mut wanted: Vec<String> = depends
        .lines()
        .filter(|line| line.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false))
        .map(|line| line.trim().to_string())
        .filter(|name| !name.contains(' ') && !name.contains(':') && !name.is_empty())
        .collect();
    wanted.sort();
    wanted.dedup();
    wanted.into_iter().filter(|name| !installed(name)).collect()
}

/// Linux (Ubuntu, Debian): the distribution's own QEMU packages, fetched with `apt-get download`
/// — which needs no sudo, and checks every package against the archive's signed index — then
/// unpacked into Colai's folder with `dpkg-deb -x`. Nothing is installed; nothing asks.
fn get_engine_linux() -> Result<(), String> {
    if Command::new("apt-get").arg("--version").output().is_err() {
        return Err("VMs on this Linux need apt (Ubuntu or Debian).".to_string());
    }
    set_build(Build::Fetching { what: "the VM engine", got: 0, of: 0 });
    let depends = run(Command::new("apt-cache").args([
        "depends",
        "--recurse",
        "--no-recommends",
        "--no-suggests",
        "--no-conflicts",
        "--no-breaks",
        "--no-replaces",
        "--no-enhances",
        "qemu-system-x86",
        "qemu-utils",
    ]))?;
    let installed = |name: &str| {
        Command::new("dpkg-query")
            .args(["-W", "-f=${Status}", name])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("install ok installed"))
            .unwrap_or(false)
    };
    let need = missing_packages(&depends, installed);
    let downloads = vms_dir().ok_or("no home folder")?.join("downloads-linux");
    let _ = fs::remove_dir_all(&downloads);
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    if !need.is_empty() {
        // Some names are virtual (provided by another package); apt skips what it cannot fetch.
        let _ = Command::new("apt-get").arg("download").args(&need).current_dir(&downloads).output();
    }
    set_build(Build::Unpacking);
    let staged = vms_dir().ok_or("no home folder")?.join("engine-new");
    let _ = fs::remove_dir_all(&staged);
    fs::create_dir_all(staged.join("root")).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&downloads).map_err(|e| e.to_string())?.flatten() {
        if entry.path().extension().map(|ext| ext == "deb").unwrap_or(false) {
            run(Command::new("dpkg-deb").arg("-x").arg(entry.path()).arg(staged.join("root")))?;
        }
    }
    let engine = engine_dir().ok_or("no home folder")?;
    let _ = fs::remove_dir_all(&engine);
    fs::rename(&staged, &engine).map_err(|e| format!("could not put the engine in place: {e}"))?;
    let _ = fs::remove_dir_all(&downloads);
    if !engine_ready() {
        return Err("the VM engine did not unpack".to_string());
    }
    Ok(())
}

/// Windows: QEMU's official build, unpacked with 7-Zip's standalone extractor.
fn get_engine_windows() -> Result<(), String> {
    let work = vms_dir().ok_or("no home folder")?.join("engine-work");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let downloads = vms_dir().ok_or("no home folder")?.join("downloads");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;

    let seven_zr = fetch_pinned(&SEVEN_ZR, &downloads, "the unpacker")?;
    let seven_zip = fetch_pinned(&SEVEN_ZIP, &downloads, "the unpacker")?;
    let qemu = fetch_pinned(&QEMU_WINDOWS, &downloads, "the VM engine")?;

    set_build(Build::Unpacking);
    let tools = work.join("7z");
    run(Command::new(&seven_zr).arg("x").arg("-y").arg(format!("-o{}", tools.display())).arg(&seven_zip))?;
    let unpacked = work.join("qemu");
    run(Command::new(tools.join("7z.exe")).arg("x").arg("-y").arg(format!("-o{}", unpacked.display())).arg(&qemu))?;

    // Keep the one emulator, the disk tool, their libraries, and the x86 firmware.
    let engine = engine_dir().ok_or("no home folder")?;
    let staged = vms_dir().ok_or("no home folder")?.join("engine-new");
    let _ = fs::remove_dir_all(&staged);
    fs::create_dir_all(staged.join("share")).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(&unpacked).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "qemu-system-x86_64.exe" || name == "qemu-img.exe" || name.ends_with(".dll") {
            copy_tree(&entry.path(), &staged.join(&name))?;
        }
    }
    for entry in fs::read_dir(unpacked.join("share")).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if firmware_kept(&name) {
            copy_tree(&entry.path(), &staged.join("share").join(&name))?;
        }
    }
    let _ = fs::remove_dir_all(&engine);
    fs::rename(&staged, &engine).map_err(|e| format!("could not put the engine in place: {e}"))?;
    let _ = fs::remove_dir_all(&work);
    for pin in [&SEVEN_ZR, &SEVEN_ZIP, &QEMU_WINDOWS] {
        let _ = fs::remove_file(downloads.join(pin.file));
    }
    if !engine_ready() {
        return Err("the VM engine did not unpack".to_string());
    }
    Ok(())
}

fn build_all(os: &'static str) -> Result<(), String> {
    BUILDING.with(|building| building.set(os));
    if !engine_ready() {
        get_engine()?;
    }
    if !base_ready_for(os) {
        if os == WINDOWS {
            build_base_windows()?;
        } else {
            build_base()?;
        }
    }
    Ok(())
}

/* ── the Windows base: Microsoft's evaluation, installed with nobody at the keyboard ─────── */

/// Microsoft's Windows Server 2025 evaluation ISO (64-bit, English): its Evaluation Center link.
/// Served over HTTPS from Microsoft's own download host; free for 180 days.
const WINDOWS_ISO: &str = "https://go.microsoft.com/fwlink/?linkid=2345730&clcid=0x409&culture=en-us&country=us";
/// The Windows disk: room for the system, updates and the work.
const WINDOWS_DISK: &str = "64G";

/// The answer file Windows Setup finds on the Colai drive: wipe the empty disk, install the
/// Standard edition with the desktop (index 2), accept Microsoft's evaluation terms, sign in
/// as Administrator by itself, and run Colai's setup script once at the first sign-in.
pub(crate) fn windows_unattend(password: &str) -> String {
    let shell = r#"publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State" processorArchitecture="amd64""#;
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="windowsPE">
    <component name="Microsoft-Windows-International-Core-WinPE" {shell}>
      <SetupUILanguage><UILanguage>en-US</UILanguage></SetupUILanguage>
      <InputLocale>en-US</InputLocale>
      <SystemLocale>en-US</SystemLocale>
      <UILanguage>en-US</UILanguage>
      <UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Setup" {shell}>
      <DiskConfiguration>
        <Disk wcm:action="add">
          <DiskID>0</DiskID>
          <WillWipeDisk>true</WillWipeDisk>
          <CreatePartitions>
            <CreatePartition wcm:action="add"><Order>1</Order><Type>Primary</Type><Extend>true</Extend></CreatePartition>
          </CreatePartitions>
          <ModifyPartitions>
            <ModifyPartition wcm:action="add"><Order>1</Order><PartitionID>1</PartitionID><Active>true</Active><Format>NTFS</Format><Label>Windows</Label><Letter>C</Letter></ModifyPartition>
          </ModifyPartitions>
        </Disk>
      </DiskConfiguration>
      <ImageInstall>
        <OSImage>
          <InstallFrom><MetaData wcm:action="add"><Key>/IMAGE/INDEX</Key><Value>2</Value></MetaData></InstallFrom>
          <InstallTo><DiskID>0</DiskID><PartitionID>1</PartitionID></InstallTo>
        </OSImage>
      </ImageInstall>
      <UserData>
        <AcceptEula>true</AcceptEula>
        <FullName>Colai</FullName>
        <Organization>Colai</Organization>
      </UserData>
    </component>
  </settings>
  <settings pass="specialize">
    <component name="Microsoft-Windows-Shell-Setup" {shell}>
      <ComputerName>colai-win</ComputerName>
      <TimeZone>UTC</TimeZone>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-Shell-Setup" {shell}>
      <UserAccounts>
        <AdministratorPassword><Value>{password}</Value><PlainText>true</PlainText></AdministratorPassword>
      </UserAccounts>
      <AutoLogon>
        <Enabled>true</Enabled>
        <Username>Administrator</Username>
        <Password><Value>{password}</Value><PlainText>true</PlainText></Password>
        <LogonCount>9999999</LogonCount>
      </AutoLogon>
      <OOBE>
        <HideEULAPage>true</HideEULAPage>
        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>
        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>
        <ProtectYourPC>3</ProtectYourPC>
        <SkipMachineOOBE>true</SkipMachineOOBE>
        <SkipUserOOBE>true</SkipUserOOBE>
      </OOBE>
      <FirstLogonCommands>
        <SynchronousCommand wcm:action="add">
          <Order>1</Order>
          <Description>Colai</Description>
          <CommandLine>powershell -NoProfile -ExecutionPolicy Bypass -Command "$d = (Get-PSDrive -PSProvider FileSystem | Where-Object {{ Test-Path (Join-Path $_.Root 'colai-setup.ps1') }} | Select-Object -First 1).Root; &amp; (Join-Path $d 'colai-setup.ps1')"</CommandLine>
        </SynchronousCommand>
      </FirstLogonCommands>
    </component>
  </settings>
</unattend>
"#
    )
}

/// Run once, at the first sign-in: SSH on (the agent's way in, with Colai's key), a machine
/// that stays awake and quiet, the evaluation activated, then off — which is the build's "done".
pub(crate) fn windows_setup_script() -> &'static str {
    r#"$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not (Get-Service sshd -ErrorAction SilentlyContinue)) {
  Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0 | Out-Null
}
Set-Service sshd -StartupType Automatic
Start-Service sshd
New-NetFirewallRule -Name colai-ssh -DisplayName 'Colai SSH' -Protocol TCP -LocalPort 22 -Action Allow -ErrorAction SilentlyContinue | Out-Null
New-Item -ItemType Directory -Force 'C:ProgramDatassh' | Out-Null
$keys = 'C:ProgramDatasshadministrators_authorized_keys'
Copy-Item (Join-Path $here 'colai.pub') $keys -Force
icacls $keys /inheritance:r /grant 'Administrators:F' /grant 'SYSTEM:F' | Out-Null
New-ItemProperty -Path 'HKLM:SOFTWAREOpenSSH' -Name DefaultShell -Value 'C:WindowsSystem32WindowsPowerShell1.0powershell.exe' -PropertyType String -Force | Out-Null
Get-ScheduledTask -TaskName ServerManager -ErrorAction SilentlyContinue | Disable-ScheduledTask | Out-Null
powercfg /change monitor-timeout-ac 0
powercfg /change standby-timeout-ac 0
cscript //B C:WindowsSystem32slmgr.vbs /ato
Stop-Computer -Force
"#
}

/// Forward slashes for QEMU's own path options, where a backslash and a drive colon can confuse.
fn qemu_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

fn build_base_windows() -> Result<(), String> {
    let qemu = find_tool(system_binary()).ok_or("the VM engine is missing")?;
    let qemu_img = find_tool("qemu-img").ok_or("the VM engine is missing")?;
    let dir = base_dir_for(WINDOWS).ok_or("no home folder")?;

    // The ISO: Microsoft's, over HTTPS. Kept until the base is built, so a retry need not
    // fetch eight gigabytes again; a partial or short file is fetched again.
    let iso = dir.join("windows-server-eval.iso");
    let want = content_length(WINDOWS_ISO);
    let have = fs::metadata(&iso).map(|m| m.len()).unwrap_or(0);
    if have == 0 || (want > 0 && have != want) {
        download(WINDOWS_ISO, &iso, "Windows")?;
    }

    set_build(Build::Installing);
    let base = dir.join("base.qcow2");
    let _ = fs::remove_file(&base);
    run(engine_command(&qemu_img).args(["create", "-f", "qcow2"]).arg(&base).arg(WINDOWS_DISK))?;

    // The Colai drive: the answer file, the first-sign-in script, and the key the agent uses.
    let drive = dir.join("colai-drive");
    let _ = fs::remove_dir_all(&drive);
    fs::create_dir_all(&drive).map_err(|e| e.to_string())?;
    if !dir.join("id_ed25519").is_file() {
        make_key(&dir, "windows")?;
    }
    let password = format!("Colai-{}!", &format!("{:x}", now_ms())[..8]);
    fs::write(drive.join("autounattend.xml"), windows_unattend(&password)).map_err(|e| e.to_string())?;
    fs::write(drive.join("colai-setup.ps1"), windows_setup_script()).map_err(|e| e.to_string())?;
    fs::copy(dir.join("id_ed25519.pub"), drive.join("colai.pub")).map_err(|e| e.to_string())?;

    let ports = free_ports()?;
    let mut boot = engine_command(&qemu);
    boot.args(qemu_args(&QemuRun {
        name: "colai-windows-base",
        disk: &base,
        cpus: 2,
        memory_mb: 4096,
        ports: &ports,
        seed: "",
        firmware: firmware_dirs(),
        os: WINDOWS,
        install: Some((iso.clone(), drive.clone())),
    }));
    crate::session::no_console_window(&mut boot);
    let log = fs::File::create(dir.join("build.log")).map_err(|e| e.to_string())?;
    let child = boot
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("could not start the VM engine: {e}"))?;
    wait_for_base(child, &base, ports.qmp, Duration::from_secs(25 * 60), &dir)?;
    fs::write(dir.join("ready"), now_ms().to_string()).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&iso);
    let _ = fs::remove_dir_all(&drive);
    Ok(())
}

fn build_base() -> Result<(), String> {
    let qemu = find_tool(system_binary()).ok_or("the VM engine is missing")?;
    let qemu_img = find_tool("qemu-img").ok_or("the VM engine is missing")?;
    let dir = base_dir().ok_or("no home folder")?;
    let image = dir.join(image_name());

    // The cloud image: fetched once, kept, and checked against Ubuntu's sums.
    if !image.is_file() {
        let download_to = dir.join(format!("{}.download", image_name()));
        download(&format!("{IMAGE_DIR}/{}", image_name()), &download_to, "Ubuntu")?;
        let sums = run(Command::new("curl").args(["-fsL", &format!("{IMAGE_DIR}/SHA256SUMS")]))?;
        let want = published_sum(&sums, image_name()).ok_or("Ubuntu's checksum list does not name the image")?;
        if hash_of(&download_to, 256)? != want {
            let _ = fs::remove_file(&download_to);
            return Err("the Ubuntu image did not match Ubuntu's checksum, so it was thrown away".to_string());
        }
        fs::rename(&download_to, &image).map_err(|e| e.to_string())?;
    }

    set_build(Build::Installing);
    let base = dir.join("base.qcow2");
    let _ = fs::remove_file(&base);
    run(engine_command(&qemu_img).args(["convert", "-O", "qcow2"]).arg(&image).arg(&base))?;
    run(engine_command(&qemu_img).arg("resize").arg(&base).arg(DISK_SIZE))?;

    // One boot with cloud-init does the installing, then powers the machine off.
    let seed_port = seed_server()?;
    register_seed("base", "colai-base", &base_user_data());
    let ports = free_ports()?;
    let mut boot = engine_command(&qemu);
    boot.args(qemu_args(&QemuRun {
        name: "colai-base",
        disk: &base,
        cpus: 2,
        memory_mb: 4096,
        ports: &ports,
        seed: &format!("http://10.0.2.2:{seed_port}/base/"),
        firmware: firmware_dirs(),
        os: UBUNTU,
        install: None,
    }));
    crate::session::no_console_window(&mut boot);
    let log = fs::File::create(dir.join("build.log")).map_err(|e| e.to_string())?;
    let child = boot
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("could not start the VM engine: {e}"))?;
    wait_for_base(child, &base, ports.qmp, Duration::from_secs(15 * 60), &dir)?;
    fs::write(dir.join("ready"), now_ms().to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Wait for a base machine to finish installing and switch itself off.
fn wait_for_base(mut child: std::process::Child, base: &Path, qmp_port: u16, still_for: Duration, dir: &Path) -> Result<(), String> {
    /*
     * Finished is the machine powering itself off. It is also a disk that has stopped changing:
     * once the installing is done and the disk has been still for a quarter of an hour, the
     * machine is asked to shut down, and switched off if it ignores that. A base that installed
     * everything and then sat at its login prompt kept "Setting up Ubuntu" on screen for hours.
     */
    let began = Instant::now();
    let footprint = || {
        fs::metadata(base)
            .map(|m| (m.len(), m.modified().ok()))
            .unwrap_or((0, None))
    };
    let mut last = footprint();
    let mut still_since = Instant::now();
    let mut asked_off: Option<Instant> = None;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if began.elapsed() > Duration::from_secs(3 * 60 * 60) {
            let _ = child.kill();
            return Err("setting up the base machine took too long and was stopped".to_string());
        }
        let now = footprint();
        if now != last {
            last = now;
            still_since = Instant::now();
        } else if still_since.elapsed() > still_for {
            match asked_off {
                None => {
                    let _ = qmp(qmp_port, "system_powerdown", None);
                    asked_off = Some(Instant::now());
                }
                Some(at) if at.elapsed() > Duration::from_secs(90) => {
                    let _ = qmp(qmp_port, "quit", None);
                }
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    };
    if !status.success() {
        let said = fs::read_to_string(dir.join("build.log")).unwrap_or_default();
        return Err(format!("the base machine did not finish: {}", said.lines().last().unwrap_or("").trim()));
    }
    Ok(())
}

/// The base machine: an XFCE desktop that signs itself in, the tools an agent builds with,
/// and SSH, which is how the agent works in it. It powers off when done.
fn base_user_data() -> String {
    r#"#cloud-config
hostname: colai-base
users:
  - name: colai
    groups: [sudo]
    shell: /bin/bash
    sudo: ALL=(ALL) NOPASSWD:ALL
    lock_passwd: true
ssh_pwauth: false
package_update: true
packages:
  - xfce4
  - xfce4-terminal
  - lightdm
  - lightdm-gtk-greeter
  - git
  - curl
  - build-essential
  - python3
  - python3-pip
  - nodejs
  - npm
  - docker.io
  - openssh-server
write_files:
  - path: /etc/lightdm/lightdm.conf.d/50-colai.conf
    content: |
      [Seat:*]
      autologin-user=colai
      autologin-session=xfce
runcmd:
  - usermod -aG docker colai
  - systemctl set-default graphical.target
power_state:
  mode: poweroff
  timeout: 1800
"#
    .to_string()
}

/// What each VM's own first boot sets: its name, and the key the toolbar reaches it with.
fn vm_user_data(hostname: &str, public_key: &str) -> String {
    format!(
        "#cloud-config\nhostname: {hostname}\nusers:\n  - name: colai\n    groups: [sudo, docker]\n    shell: /bin/bash\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    lock_passwd: true\n    ssh_authorized_keys:\n      - {}\nssh_pwauth: false\n",
        public_key.trim()
    )
}

/* ── the seed server: cloud-init's NoCloud, over HTTP, from 127.0.0.1 ───────────────────── */

type SeedMap = std::collections::HashMap<String, (String, String)>;

fn seeds() -> &'static Mutex<SeedMap> {
    static SEEDS: OnceLock<Mutex<SeedMap>> = OnceLock::new();
    SEEDS.get_or_init(|| Mutex::new(SeedMap::new()))
}

fn register_seed(id: &str, hostname: &str, user_data: &str) {
    let meta = format!("instance-id: {id}\nlocal-hostname: {hostname}\n");
    if let Ok(mut held) = seeds().lock() {
        held.insert(id.to_string(), (meta, user_data.to_string()));
    }
}

/// Start (once) the little HTTP server cloud-init reads its seed from. The guest reaches the
/// host's loopback as 10.0.2.2, so it listens on 127.0.0.1 only.
fn seed_server() -> Result<u16, String> {
    static PORT: OnceLock<u16> = OnceLock::new();
    if let Some(port) = PORT.get() {
        return Ok(*port);
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("could not serve the VM seed: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve_seed(stream));
        }
    });
    let _ = PORT.set(port);
    Ok(*PORT.get().unwrap_or(&port))
}

fn serve_seed(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let (status, body) = match seed_answer(path) {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", String::new()),
    };
    let _ = write!(
        stream,
        "HTTP/1.0 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// `/<id>/meta-data`, `/<id>/user-data`, `/<id>/vendor-data` — nothing else.
fn seed_answer(path: &str) -> Option<String> {
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let id = parts.next()?;
    let file = parts.next()?;
    let held = seeds().lock().ok()?;
    let (meta, user) = held.get(id)?;
    match file {
        "meta-data" => Some(meta.clone()),
        "user-data" => Some(user.clone()),
        "vendor-data" => Some(String::new()),
        _ => None,
    }
}

/* ── running QEMU ──────────────────────────────────────────────────────────────────────── */

fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no free port: {e}"))?;
    listener.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

/// Four ports on 127.0.0.1. The VNC one is a display number, so it is looked for from 5901 up.
fn free_ports() -> Result<Ports, String> {
    let vnc_display = (1..100u16)
        .find(|n| TcpListener::bind(("127.0.0.1", 5900 + n)).is_ok())
        .ok_or("no free VNC display")?;
    Ok(Ports { vnc_display, ws: free_port()?, qmp: free_port()?, ssh: free_port()? })
}

pub(crate) struct QemuRun<'a> {
    pub name: &'a str,
    pub disk: &'a Path,
    pub cpus: u32,
    pub memory_mb: u32,
    pub ports: &'a Ports,
    pub seed: &'a str,
    /// Where the bundled engine keeps its firmware (`-L`, in order), when it is Colai's own.
    pub firmware: Vec<PathBuf>,
    /// "ubuntu" or "windows": Windows gets hardware it has drivers for out of the box.
    pub os: &'a str,
    /// Installing Windows: the ISO to boot once, and the folder shown as the Colai drive.
    pub install: Option<(PathBuf, PathBuf)>,
}

/// QEMU's arguments for one machine on this host. Everything it listens on is 127.0.0.1.
pub(crate) fn qemu_args(run: &QemuRun) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let push = |args: &mut Vec<String>, items: &[&str]| args.extend(items.iter().map(|s| s.to_string()));
    push(&mut args, &["-name", run.name]);
    for firmware in &run.firmware {
        push(&mut args, &["-L", &firmware.display().to_string()]);
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        push(&mut args, &["-machine", "virt", "-accel", "hvf", "-cpu", "host"]);
    } else if cfg!(target_os = "macos") {
        push(&mut args, &["-machine", "q35", "-accel", "hvf", "-accel", "tcg"]);
    } else if cfg!(windows) {
        // Windows' hypervisor when it is on, plain emulation when it is not. Never a prompt.
        push(&mut args, &["-machine", "q35", "-accel", "whpx,kernel-irqchip=off", "-accel", "tcg"]);
    } else {
        push(&mut args, &["-machine", "q35", "-accel", "kvm", "-accel", "tcg", "-cpu", "host"]);
    }
    let smp = run.cpus.to_string();
    let mem = run.memory_mb.to_string();
    let net = format!("user,id=n0,hostfwd=tcp:127.0.0.1:{}-:22", run.ports.ssh);
    let vnc = format!("127.0.0.1:{},websocket={}", run.ports.vnc_display, run.ports.ws);
    let qmp = format!("tcp:127.0.0.1:{},server=on,wait=off", run.ports.qmp);
    push(&mut args, &["-smp", &smp, "-m", &mem]);
    if run.os == WINDOWS {
        // SATA, Intel networking and standard VGA: Windows drives them with no extra drivers.
        let disk = format!("file={},if=none,id=d0,format=qcow2", qemu_path(run.disk));
        push(&mut args, &["-drive", &disk, "-device", "ide-hd,drive=d0,bus=ide.0"]);
        push(&mut args, &["-netdev", &net, "-device", "e1000,netdev=n0"]);
        push(&mut args, &["-vga", "std", "-device", "qemu-xhci", "-device", "usb-tablet", "-device", "usb-kbd"]);
        if let Some((iso, drive)) = &run.install {
            let cd = format!("file={},if=none,id=cd0,media=cdrom,readonly=on", qemu_path(iso));
            let colai = format!("if=none,id=u0,format=raw,file=fat:ro:{}", qemu_path(drive));
            push(&mut args, &["-drive", &cd, "-device", "ide-cd,drive=cd0,bus=ide.1"]);
            push(&mut args, &["-drive", &colai, "-device", "usb-storage,drive=u0,removable=on"]);
            push(&mut args, &["-boot", "once=d"]);
        }
    } else {
        let disk = format!("file={},if=virtio,format=qcow2", run.disk.display());
        let smbios = format!("type=1,serial=ds=nocloud;s={}", run.seed);
        push(&mut args, &["-drive", &disk]);
        push(&mut args, &["-netdev", &net, "-device", "virtio-net-pci,netdev=n0"]);
        push(&mut args, &["-device", "virtio-vga", "-device", "qemu-xhci", "-device", "usb-tablet", "-device", "usb-kbd"]);
        push(&mut args, &["-smbios", &smbios]);
    }
    push(&mut args, &["-display", "none", "-vnc", &vnc, "-qmp", &qmp]);
    args
}

/* ── QMP: asking a running machine ─────────────────────────────────────────────────────── */

fn qmp(port: u16, command: &str, arguments: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let addr = format!("127.0.0.1:{port}").parse().map_err(|_| "bad address".to_string())?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(400)).map_err(|_| "not running".to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut writer = stream;
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?; // the greeting
    let mut ask = |value: serde_json::Value| -> Result<serde_json::Value, String> {
        writeln!(writer, "{value}").map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                return Err("the machine closed the connection".to_string());
            }
            let said: serde_json::Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if let Some(answer) = said.get("return") {
                return Ok(answer.clone());
            }
            if let Some(error) = said.get("error") {
                return Err(error.get("desc").and_then(|d| d.as_str()).unwrap_or("QMP error").to_string());
            }
        }
    };
    ask(serde_json::json!({ "execute": "qmp_capabilities" }))?;
    let mut message = serde_json::json!({ "execute": command });
    if let Some(arguments) = arguments {
        message["arguments"] = arguments;
    }
    ask(message)
}

fn is_running(vm: &Vm) -> bool {
    vm.ports.as_ref().map(|ports| qmp(ports.qmp, "query-status", None).is_ok()).unwrap_or(false)
}

/* ── commands ──────────────────────────────────────────────────────────────────────────── */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Setup {
    /// Ubuntu: "ready", "building" (with `said`), "missing" (nothing done yet), or "failed".
    pub state: String,
    pub said: String,
    /// The same, for the Windows evaluation.
    pub windows: SystemSetup,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SystemSetup {
    pub state: String,
    pub said: String,
}

/// How far this computer is from running a VM of each system. Nothing here is for the person.
#[tauri::command]
pub(crate) fn colai_vm_setup() -> Setup {
    let ubuntu = setup_of(UBUNTU);
    Setup { state: ubuntu.state, said: ubuntu.said, windows: setup_of(WINDOWS) }
}

fn setup_of(os: &'static str) -> SystemSetup {
    if engine_ready() && base_ready_for(os) {
        return SystemSetup { state: "ready".to_string(), said: String::new() };
    }
    let (state, said) = match build_state_of(os) {
        Build::Idle => ("missing", String::new()),
        Build::Fetching { what, got, of } => (
            "building",
            if of > 0 {
                format!("Downloading {what} · {} of {} MB", got / 1_048_576, (of / 1_048_576).max(1))
            } else {
                format!("Downloading {what} · {} MB", got / 1_048_576)
            },
        ),
        Build::Unpacking => ("building", "Unpacking the VM engine".to_string()),
        Build::Installing => (
            "building",
            if os == WINDOWS { "Installing Windows".to_string() } else { "Setting up Ubuntu".to_string() },
        ),
        Build::Failed(why) => ("failed", why),
    };
    SystemSetup { state: state.to_string(), said }
}

/// Get everything ready — the engine, then the base machine — in the background. The page
/// starts this itself the first time the VM key is opened, and polls `colai_vm_setup`.
#[tauri::command]
pub(crate) fn colai_vm_build(os: Option<String>) -> Result<(), String> {
    let os = os_of(os.as_deref().unwrap_or(UBUNTU)).ok_or("not a system")?;
    if matches!(build_state_of(os), Build::Fetching { .. } | Build::Unpacking | Build::Installing) {
        return Ok(());
    }
    if let Ok(mut held) = build_states().lock() {
        held.insert(os, Build::Fetching { what: "the VM engine", got: 0, of: 0 });
    }
    std::thread::spawn(move || match build_all(os) {
        Ok(()) => set_build(Build::Idle),
        Err(why) => set_build(Build::Failed(why)),
    });
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VmRow {
    #[serde(flatten)]
    pub vm: Vm,
    pub running: bool,
}

#[tauri::command]
pub(crate) async fn colai_vm_list() -> Result<Vec<VmRow>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        all_vms()
            .into_iter()
            .map(|vm| {
                let running = is_running(&vm);
                VmRow { vm, running }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// The key the toolbar reaches a machine with: an ed25519 pair made by `ssh-keygen`, which
/// Windows 10 and later, macOS and Linux all ship.
fn make_key(dir: &Path, id: &str) -> Result<(), String> {
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C"])
        .arg(format!("colai-{id}"))
        .arg("-f")
        .arg(dir.join("id_ed25519")))
    .map(|_| ())
}

/// A new machine: an overlay on the base, its own SSH key, and (optionally) its owner.
#[tauri::command]
pub(crate) fn colai_vm_create(
    name: String,
    size: String,
    os: Option<String>,
    owner: Option<String>,
    owner_title: Option<String>,
) -> Result<Vm, String> {
    let os = os_of(os.as_deref().unwrap_or(UBUNTU)).ok_or("not a system")?;
    if !engine_ready() || !base_ready_for(os) {
        return Err("Still getting VMs ready.".to_string());
    }
    let (cpus, memory_mb) = size_of(&size).ok_or("not a VM size")?;
    // Windows with a desktop needs room: never less than 4 GB.
    let memory_mb = if os == WINDOWS { memory_mb.max(4096) } else { memory_mb };
    let name = name.trim();
    if name.is_empty() || name.len() > 40 {
        return Err("Give the VM a name.".to_string());
    }
    let owner = owner.filter(|o| !o.is_empty());
    if let Some(owner) = &owner {
        if let Some(other) = all_vms().into_iter().find(|v| v.owner.as_deref() == Some(owner)) {
            return Err(format!("That conversation already has {}.", other.name));
        }
    }
    let qemu_img = find_tool("qemu-img").ok_or("the VM engine is missing")?;
    let created_at = now_ms();
    let id = new_id(created_at);
    let dir = vm_dir(&id).ok_or("no home folder")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = base_image_for(os).ok_or("no base image")?;
    run(engine_command(&qemu_img)
        .args(["create", "-f", "qcow2", "-F", "qcow2", "-b"])
        .arg(&base)
        .arg(dir.join("disk.qcow2")))?;
    if os == WINDOWS {
        // Windows has no cloud-init to take a key of its own at first boot, so its machines
        // share the key their base was built with — still Colai's alone, never the person's.
        let from = base_dir_for(WINDOWS).ok_or("no base")?;
        for file in ["id_ed25519", "id_ed25519.pub"] {
            fs::copy(from.join(file), dir.join(file)).map_err(|e| e.to_string())?;
        }
    } else {
        make_key(&dir, &id)?;
    }
    let vm = Vm {
        id,
        name: name.to_string(),
        size,
        cpus,
        memory_mb,
        owner,
        owner_title,
        created_at,
        ports: None,
        pid: None,
        saved_at: None,
        os: os.to_string(),
    };
    write_vm(&vm)?;
    Ok(vm)
}

fn hostname_of(name: &str) -> String {
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() { "colai-vm".to_string() } else { slug.chars().take(30).collect() }
}

fn start(id: &str) -> Result<Vm, String> {
    let mut vm = read_vm(id).ok_or("no such VM")?;
    if is_running(&vm) {
        return Ok(vm);
    }
    let qemu = find_tool(system_binary()).ok_or("the VM engine is missing")?;
    let dir = vm_dir(id).ok_or("no such VM")?;
    let key = fs::read_to_string(dir.join("id_ed25519.pub")).unwrap_or_default();
    let seed_port = seed_server()?;
    if vm.os != WINDOWS {
        register_seed(id, &hostname_of(&vm.name), &vm_user_data(&hostname_of(&vm.name), &key));
    }
    let ports = free_ports()?;
    let disk = dir.join("disk.qcow2");
    let mut cmd = engine_command(&qemu);
    cmd.args(qemu_args(&QemuRun {
        name: &format!("colai-{}", hostname_of(&vm.name)),
        disk: &disk,
        cpus: vm.cpus,
        memory_mb: vm.memory_mb,
        ports: &ports,
        seed: &format!("http://10.0.2.2:{seed_port}/{id}/"),
        firmware: firmware_dirs(),
        os: if vm.os == WINDOWS { WINDOWS } else { UBUNTU },
        install: None,
    }));
    crate::session::no_console_window(&mut cmd);
    let log = fs::File::create(dir.join("qemu.log")).map_err(|e| e.to_string())?;
    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("could not start the VM: {e}"))?;
    vm.pid = Some(child.id());
    vm.ports = Some(ports);
    write_vm(&vm)?;
    let _ = write_ssh_config();
    // Up when QMP answers. A QEMU that exits at once says why in its log.
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(20) {
        if is_running(&vm) {
            return Ok(vm);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let said = fs::read_to_string(dir.join("qemu.log")).unwrap_or_default();
    vm.ports = None;
    vm.pid = None;
    let _ = write_vm(&vm);
    Err(format!("the VM did not start: {}", said.lines().last().unwrap_or("no reason given")))
}

#[tauri::command]
pub(crate) async fn colai_vm_start(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start(&id).map(|_| ()))
        .await
        .map_err(|e| e.to_string())?
}

/// Ask it to shut down; pull the plug if it has not after 20 seconds.
fn stop(id: &str) -> Result<(), String> {
    let mut vm = read_vm(id).ok_or("no such VM")?;
    if let Some(ports) = vm.ports.clone() {
        if qmp(ports.qmp, "system_powerdown", None).is_ok() {
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(20) && is_running(&vm) {
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        if is_running(&vm) {
            let _ = qmp(ports.qmp, "quit", None);
        }
    }
    vm.ports = None;
    vm.pid = None;
    write_vm(&vm)
}

/*
 * Save points.
 *
 * One per machine: "save point" keeps the disk as it is now, "back to save point" puts it back,
 * "reset to clean" starts the machine over from the base. Each is done on a machine that has
 * been shut down cleanly — QEMU's own snapshot of a qcow2 disk, never a copy — and the machine
 * is started again if it was running, so the window it is shown in simply reconnects.
 */
const SAVE_POINT: &str = "colai-save-point";

fn with_machine_off(id: &str, change: impl FnOnce(&mut Vm, &Path, &Path) -> Result<(), String>) -> Result<(), String> {
    let was_running = read_vm(id).map(|vm| is_running(&vm)).unwrap_or(false);
    stop(id)?;
    let mut vm = read_vm(id).ok_or("no such VM")?;
    let qemu_img = find_tool("qemu-img").ok_or("the VM engine is missing")?;
    let disk = vm_dir(id).ok_or("no such VM")?.join("disk.qcow2");
    change(&mut vm, &qemu_img, &disk)?;
    write_vm(&vm)?;
    if was_running {
        start(id)?;
    }
    Ok(())
}

fn save(id: &str) -> Result<(), String> {
    with_machine_off(id, |vm, qemu_img, disk| {
        if vm.saved_at.is_some() {
            let _ = run(engine_command(qemu_img).args(["snapshot", "-d", SAVE_POINT]).arg(disk));
        }
        run(engine_command(qemu_img).args(["snapshot", "-c", SAVE_POINT]).arg(disk))?;
        vm.saved_at = Some(now_ms());
        Ok(())
    })
}

fn restore(id: &str) -> Result<(), String> {
    with_machine_off(id, |vm, qemu_img, disk| {
        if vm.saved_at.is_none() {
            return Err("There is no save point yet.".to_string());
        }
        run(engine_command(qemu_img).args(["snapshot", "-a", SAVE_POINT]).arg(disk)).map(|_| ())
    })
}

fn reset(id: &str) -> Result<(), String> {
    with_machine_off(id, |vm, qemu_img, disk| {
        let base = base_image_for(&vm.os).ok_or("no base image")?;
        fs::remove_file(disk).map_err(|e| format!("could not clear the disk: {e}"))?;
        run(engine_command(qemu_img)
            .args(["create", "-f", "qcow2", "-F", "qcow2", "-b"])
            .arg(&base)
            .arg(disk))?;
        // A fresh disk has never seen this machine's host key: forget the old one.
        if let Some(dir) = disk.parent() {
            let _ = fs::remove_file(dir.join("known_hosts"));
        }
        vm.saved_at = None;
        Ok(())
    })
}

#[tauri::command]
pub(crate) async fn colai_vm_save(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save(&id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn colai_vm_restore(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || restore(&id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn colai_vm_reset(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reset(&id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn colai_vm_stop(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || stop(&id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn colai_vm_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window(&window_label(&id)) {
        let _ = window.close();
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _ = stop(&id);
        let dir = vm_dir(&id).ok_or("no such VM")?;
        fs::remove_dir_all(dir).map_err(|e| format!("could not delete it: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/*
 * The agent in its machine.
 *
 * The agent is the conversation the VM belongs to — the person's own Claude session, signed in
 * as itself — and it works in the VM over SSH, with the VM's own key. Nothing signs in inside
 * the machine, and no credential of the person's is ever copied into it. Each message to that
 * conversation carries a short brief saying so, with the exact commands; the machine is woken
 * first if it was stopped, and its SSH line written down where the commands point.
 */

/// The SSH config every running VM is reachable through, as `ssh -F <this> <alias>`.
fn ssh_config_path() -> Option<PathBuf> {
    vms_dir().map(|dir| dir.join("ssh_config"))
}

/// The alias a VM is reached by: stable across restarts, though its port is not.
pub(crate) fn alias_of(vm: &Vm) -> String {
    format!("colai-{}", hostname_of(&vm.name))
}

/// Forward slashes: OpenSSH reads them on every system, and a backslash is an escape to it.
fn ssh_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

pub(crate) fn ssh_config_text(vms: &[(Vm, PathBuf)]) -> String {
    let mut text = String::from("# Written by Colai: one entry per running VM. Rewritten as they start.\n");
    for (vm, dir) in vms {
        let Some(ports) = &vm.ports else { continue };
        text.push_str(&format!(
            "\nHost {}\n  HostName 127.0.0.1\n  Port {}\n  User {}\n  IdentityFile \"{}\"\n  IdentitiesOnly yes\n  StrictHostKeyChecking accept-new\n  UserKnownHostsFile \"{}\"\n  ConnectTimeout 10\n",
            alias_of(vm),
            ports.ssh,
            if vm.os == WINDOWS { "Administrator" } else { "colai" },
            ssh_path(&dir.join("id_ed25519")),
            ssh_path(&dir.join("known_hosts")),
        ));
    }
    text
}

fn write_ssh_config() -> Result<PathBuf, String> {
    let path = ssh_config_path().ok_or("no home folder")?;
    let running: Vec<(Vm, PathBuf)> = all_vms()
        .into_iter()
        .filter(|vm| vm.ports.is_some())
        .filter_map(|vm| vm_dir(&vm.id).map(|dir| (vm, dir)))
        .collect();
    fs::write(&path, ssh_config_text(&running)).map_err(|e| format!("could not write the SSH line: {e}"))?;
    Ok(path)
}

pub(crate) fn brief_text(vm: &Vm, config: &Path) -> String {
    let config = ssh_path(config);
    let alias = alias_of(vm);
    if vm.os == WINDOWS {
        return format!(
            "[Colai] This conversation has its own virtual machine, \"{name}\": Windows Server 2025 (evaluation) with a desktop, {cpus} CPU and {gb} GB, running on this computer and used by nobody else. Do the building, testing and deploying for this work inside it, not on this computer.\n\
             - Run a command in it (PowerShell): ssh -F \"{config}\" {alias} '<command>'\n\
             - Copy files in: scp -F \"{config}\" <local path> {alias}:<path>  (and the other way to copy out)\n\
             - You are Administrator. Install what the work needs (for example with Invoke-WebRequest); clone the project into it rather than working on this computer's copy.\n\
             - If it has only just started, SSH may refuse for a minute while Windows boots: wait and try again.\n\
             The person can watch its screen live in Colai's VM window.",
            name = vm.name,
            cpus = vm.cpus,
            gb = vm.memory_mb / 1024,
        );
    }
    format!(
        "[Colai] This conversation has its own virtual machine, \"{name}\": Ubuntu 24.04 with a desktop, {cpus} CPU and {gb} GB, running on this computer and used by nobody else. Do the building, testing and deploying for this work inside it, not on this computer.\n\
         - Run a command in it: ssh -F \"{config}\" {alias} '<command>'\n\
         - Copy files in: scp -F \"{config}\" <local path> {alias}:<path>  (and the other way to copy out)\n\
         - It has git, node, npm, python3, build tools and docker; the user is colai, with passwordless sudo. Clone the project into it rather than working on this computer's copy.\n\
         - If it has only just started, SSH may refuse for a few seconds while it boots: wait and try again.\n\
         The person can watch its screen live in Colai's VM window.",
        name = vm.name,
        cpus = vm.cpus,
        gb = vm.memory_mb / 1024,
    )
}

/// The brief for a message to `owner`, if that conversation has a VM: woken, reachable, said.
pub(crate) fn brief_for(owner: &str) -> Option<String> {
    let vm = all_vms().into_iter().find(|vm| vm.owner.as_deref() == Some(owner))?;
    let vm = start(&vm.id).ok()?;
    let config = write_ssh_config().ok()?;
    Some(brief_text(&vm, &config))
}

/// Give the machine to a conversation, or (with no owner) free it. One machine per conversation.
#[tauri::command]
pub(crate) fn colai_vm_bind(id: String, owner: Option<String>, owner_title: Option<String>) -> Result<Vm, String> {
    let mut vm = read_vm(&id).ok_or("no such VM")?;
    let owner = owner.filter(|o| !o.is_empty());
    if let Some(owner) = &owner {
        if let Some(other) = all_vms().into_iter().find(|v| v.id != id && v.owner.as_deref() == Some(owner)) {
            return Err(format!("That conversation already has {}.", other.name));
        }
    }
    vm.owner_title = owner.as_ref().and(owner_title);
    vm.owner = owner;
    write_vm(&vm)?;
    Ok(vm)
}

/// A PNG of its screen, as a data URL, for the card in the popup.
#[tauri::command]
pub(crate) async fn colai_vm_thumb(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let vm = read_vm(&id).ok_or("no such VM")?;
        let ports = vm.ports.ok_or("not running")?;
        let dir = vm_dir(&id).ok_or("no such VM")?;
        let shot = dir.join("thumb.png");
        qmp(
            ports.qmp,
            "screendump",
            Some(serde_json::json!({ "filename": shot.display().to_string(), "format": "png" })),
        )?;
        let bytes = fs::read(&shot).map_err(|e| e.to_string())?;
        use base64::Engine;
        Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What the VM window needs: where its screen is.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VmScreen {
    pub name: String,
    pub ws: u16,
    pub saved_at: Option<u64>,
    pub owner_title: Option<String>,
    pub cpus: u32,
    pub memory_mb: u32,
}

#[tauri::command]
pub(crate) fn colai_vm_screen(id: String) -> Result<VmScreen, String> {
    let vm = read_vm(&id).ok_or("no such VM")?;
    let ports = vm.ports.clone().filter(|_| is_running(&vm)).ok_or("The VM is not running.")?;
    Ok(VmScreen {
        name: vm.name,
        ws: ports.ws,
        saved_at: vm.saved_at,
        owner_title: vm.owner_title,
        cpus: vm.cpus,
        memory_mb: vm.memory_mb,
    })
}

fn window_label(id: &str) -> String {
    format!("colai-vm-{id}")
}

/// Start it if need be, and show its screen in a window of its own.
#[tauri::command]
pub(crate) async fn colai_vm_open(app: tauri::AppHandle, id: String) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let started = id.clone();
    let vm = tauri::async_runtime::spawn_blocking(move || start(&started))
        .await
        .map_err(|e| e.to_string())??;
    let label = window_label(&id);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(format!("vm.html?id={id}").into()))
        .title(format!("{} — Colai VM", vm.name))
        .inner_size(1120.0, 760.0)
        .min_inner_size(640.0, 420.0)
        .center()
        .resizable(true)
        .focused(true)
        .build()
        .map(|_| ())
        .map_err(|e| format!("could not open the VM window: {e}"))
}

#[cfg(test)]
mod a_vm {
    use super::*;

    fn ports() -> Ports {
        Ports { vnc_display: 3, ws: 41000, qmp: 41001, ssh: 41002 }
    }

    fn args(firmware: Vec<PathBuf>) -> Vec<String> {
        qemu_args(&QemuRun {
            name: "t",
            disk: Path::new("disk.qcow2"),
            cpus: 2,
            memory_mb: 4096,
            ports: &ports(),
            seed: "http://10.0.2.2:5/vm-1/",
            firmware,
            os: UBUNTU,
            install: None,
        })
    }

    #[test]
    fn everything_it_listens_on_is_this_machine_only() {
        let joined = args(Vec::new()).join(" ");
        assert!(joined.contains("-vnc 127.0.0.1:3,websocket=41000"), "{joined}");
        assert!(joined.contains("-qmp tcp:127.0.0.1:41001,server=on,wait=off"), "{joined}");
        assert!(joined.contains("hostfwd=tcp:127.0.0.1:41002-:22"), "{joined}");
        assert!(joined.contains("-smp 2 -m 4096"), "{joined}");
        assert!(joined.contains("serial=ds=nocloud;s=http://10.0.2.2:5/vm-1/"), "{joined}");
        assert!(!joined.contains("0.0.0.0"));
    }

    #[test]
    fn the_bundled_engine_brings_its_own_firmware() {
        let joined = args(vec![PathBuf::from("fw"), PathBuf::from("bios")]).join(" ");
        assert!(joined.contains("-L fw -L bios"), "{joined}");
        assert!(!args(Vec::new()).join(" ").contains("-L "));
    }

    #[test]
    fn the_hypervisor_is_used_when_there_and_emulation_is_behind_it() {
        let all = args(Vec::new());
        if cfg!(windows) {
            assert!(all.iter().any(|a| a.starts_with("whpx")));
            assert!(all.iter().any(|a| a == "tcg"));
        }
    }

    #[test]
    fn only_x86_firmware_is_kept_from_the_engine() {
        assert!(firmware_kept("bios-256k.bin"));
        assert!(firmware_kept("vgabios-virtio.bin"));
        assert!(firmware_kept("keymaps"));
        assert!(firmware_kept("edk2-x86_64-code.fd"));
        assert!(!firmware_kept("edk2-riscv-code.fd"));
        assert!(!firmware_kept("openbios-ppc"));
    }

    #[test]
    fn every_engine_file_is_pinned_by_hash() {
        for pin in [&SEVEN_ZR, &SEVEN_ZIP, &QEMU_WINDOWS] {
            assert!(pin.sha256.map(|h| h.len() == 64).unwrap_or(false) || pin.sha512.map(|h| h.len() == 128).unwrap_or(false));
            assert!(pin.urls.iter().all(|url| url.starts_with("https://")));
        }
    }

    #[test]
    fn only_the_three_sizes_exist() {
        assert_eq!(size_of("medium"), Some((2, 4096)));
        assert_eq!(size_of("huge"), None);
    }

    #[test]
    fn an_id_from_the_page_is_never_a_path() {
        assert!(vm_dir("../evil").is_none());
        assert!(vm_dir("vm-1/../../x").is_none());
        assert!(vm_dir("").is_none());
    }

    #[test]
    fn the_seed_answers_only_its_three_files() {
        register_seed("vm-test", "box", "#cloud-config\n");
        assert!(seed_answer("/vm-test/meta-data").unwrap().contains("instance-id: vm-test"));
        assert_eq!(seed_answer("/vm-test/user-data").unwrap(), "#cloud-config\n");
        assert_eq!(seed_answer("/vm-test/vendor-data").unwrap(), "");
        assert!(seed_answer("/vm-test/../../etc").is_none());
        assert!(seed_answer("/nobody/user-data").is_none());
    }

    #[test]
    fn each_vm_is_reached_by_its_own_key_and_never_a_password() {
        let data = vm_user_data("box", "ssh-ed25519 AAAA colai-vm-1\n");
        assert!(data.contains("- ssh-ed25519 AAAA colai-vm-1"));
        assert!(data.contains("ssh_pwauth: false"));
        assert!(base_user_data().contains("ssh_pwauth: false"));
        assert!(base_user_data().contains("lock_passwd: true"));
        // Cleaning cloud-init's state mid-run took the power-off at the end with it.
        assert!(!base_user_data().contains("cloud-init clean"));
        assert!(base_user_data().contains("mode: poweroff"));
    }

    #[test]
    fn ubuntus_published_sum_is_read_by_file_name() {
        let listing = "aaa *other.img\nBBB *noble-server-cloudimg-amd64.img\n";
        assert_eq!(published_sum(listing, "noble-server-cloudimg-amd64.img").as_deref(), Some("bbb"));
        assert_eq!(published_sum(listing, "missing.img"), None);
    }

    fn sample(name: &str, ssh: u16) -> Vm {
        Vm {
            id: "vm-1".to_string(),
            name: name.to_string(),
            size: "medium".to_string(),
            cpus: 2,
            memory_mb: 4096,
            owner: Some("chat-1".to_string()),
            owner_title: None,
            created_at: 1,
            ports: Some(Ports { vnc_display: 1, ws: 2, qmp: 3, ssh }),
            pid: None,
            saved_at: None,
            os: UBUNTU.to_string(),
        }
    }

    #[test]
    fn the_ssh_line_is_this_machine_its_own_key_and_no_password() {
        let text = ssh_config_text(&[(sample("Build Box", 2222), PathBuf::from(r"C:\Users\x\vm-1"))]);
        assert!(text.contains("Host colai-build-box"));
        assert!(text.contains("HostName 127.0.0.1"));
        assert!(text.contains("Port 2222"));
        assert!(text.contains("IdentitiesOnly yes"));
        assert!(text.contains("C:/Users/x/vm-1/id_ed25519"), "{text}");
        assert!(!text.contains('\\'), "backslashes are escapes to ssh: {text}");
    }

    #[test]
    fn a_stopped_vm_has_no_ssh_line() {
        let mut vm = sample("idle", 1);
        vm.ports = None;
        assert!(!ssh_config_text(&[(vm, PathBuf::from("d"))]).contains("Host "));
    }

    #[test]
    fn the_brief_says_where_to_work_and_how() {
        let brief = brief_text(&sample("box", 2222), Path::new("C:/cfg"));
        assert!(brief.contains("ssh -F \"C:/cfg\" colai-box"));
        assert!(brief.contains("scp -F \"C:/cfg\""));
        assert!(brief.contains("not on this computer"));
    }

    #[test]
    fn only_what_the_system_lacks_is_fetched() {
        let depends = "qemu-system-x86\n  Depends: libc6\n  Depends: <libfoo>\nlibc6\nseabios\nqemu-utils\nlibslirp0\n";
        let need = missing_packages(depends, |name| name == "libc6");
        assert_eq!(need, vec!["libslirp0", "qemu-system-x86", "qemu-utils", "seabios"]);
    }

    fn windows_args(install: bool) -> Vec<String> {
        qemu_args(&QemuRun {
            name: "w",
            disk: Path::new(r"C:\vms\disk.qcow2"),
            cpus: 2,
            memory_mb: 4096,
            ports: &Ports { vnc_display: 1, ws: 2, qmp: 3, ssh: 4 },
            seed: "",
            firmware: Vec::new(),
            os: WINDOWS,
            install: install.then(|| (PathBuf::from(r"C:\vms\server.iso"), PathBuf::from(r"C:\vms\drive"))),
        })
    }

    #[test]
    fn windows_gets_hardware_it_has_drivers_for() {
        let joined = windows_args(false).join(" ");
        assert!(joined.contains("ide-hd,drive=d0"), "{joined}");
        assert!(joined.contains("e1000,netdev=n0"), "{joined}");
        assert!(joined.contains("-vga std"), "{joined}");
        assert!(!joined.contains("virtio"), "{joined}");
        assert!(!joined.contains("-smbios"), "no cloud-init seed for Windows: {joined}");
        assert!(joined.contains("hostfwd=tcp:127.0.0.1:4-:22"));
    }

    #[test]
    fn installing_windows_boots_the_iso_once_with_the_colai_drive() {
        let joined = windows_args(true).join(" ");
        assert!(joined.contains("file=C:/vms/server.iso,if=none,id=cd0,media=cdrom,readonly=on"), "{joined}");
        assert!(joined.contains("fat:ro:C:/vms/drive"), "{joined}");
        assert!(joined.contains("-boot once=d"), "{joined}");
        assert!(!windows_args(false).join(" ").contains("cdrom"));
    }

    #[test]
    fn the_windows_answer_file_installs_with_nobody_at_the_keyboard() {
        let xml = windows_unattend("Pw-1!");
        assert!(xml.contains("<AcceptEula>true</AcceptEula>"));
        assert!(xml.contains("<Key>/IMAGE/INDEX</Key><Value>2</Value>"));
        assert!(xml.contains("<Username>Administrator</Username>"));
        assert!(xml.contains("<Value>Pw-1!</Value>"));
        assert!(xml.contains("colai-setup.ps1"));
        assert!(!xml.contains("{{"), "braces are escaped correctly");
        let script = windows_setup_script();
        assert!(script.contains("administrators_authorized_keys"));
        assert!(script.contains("Start-Service sshd"));
        assert!(script.contains("Stop-Computer"));
    }

    #[test]
    fn a_windows_vm_is_reached_as_administrator_and_briefed_in_its_own_terms() {
        let mut vm = sample("win", 2200);
        vm.os = WINDOWS.to_string();
        let text = ssh_config_text(&[(vm.clone(), PathBuf::from("d"))]);
        assert!(text.contains("User Administrator"));
        let brief = brief_text(&vm, Path::new("C:/cfg"));
        assert!(brief.contains("Windows Server 2025"));
        assert!(brief.contains("PowerShell"));
    }

    #[test]
    fn machines_made_before_the_choice_are_ubuntu() {
        let old = r#"{"id":"vm-1","name":"a","size":"medium","cpus":2,"memoryMb":4096,"createdAt":1}"#;
        let vm: Vm = serde_json::from_str(old).unwrap();
        assert_eq!(vm.os, UBUNTU);
    }

    #[test]
    fn a_hostname_is_made_safe() {
        assert_eq!(hostname_of("Checkout Redesign!"), "checkout-redesign");
        assert_eq!(hostname_of("!!!"), "colai-vm");
    }
}
