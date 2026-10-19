// A VM's screen, live: Colai's own engine (QEMU) serves it as VNC over a websocket on
// 127.0.0.1, and noVNC draws it.
//
// "Agent drives" is view-only — the agent has the machine, and a stray click here cannot get
// in its way. "You drive" hands this window the mouse and keyboard.
import RFB from "./vendor/novnc/core/rfb.js";

const invoke = window.__TAURI__.core.invoke;
const id = new URLSearchParams(location.search).get("id") || "";
const $ = (name) => document.getElementById(name);

let rfb = null;
let driving = false;
// Whether this window has connected once: after that, a reconnect keeps whoever was driving.
let connected = false;

function say(text) {
  $("said").hidden = !text;
  $("said").textContent = text || "";
}

function drive(you) {
  driving = you;
  $("agent").dataset.on = String(!you);
  $("you").dataset.on = String(you);
  if (rfb) {
    rfb.viewOnly = !you;
    if (you) rfb.focus();
  }
}

async function connect() {
  let screen;
  try {
    screen = await invoke("colai_vm_screen", { id: id });
  } catch (trouble) {
    // Stopped for a save point or a reset, or still starting: look again shortly.
    say(String(trouble));
    setTimeout(connect, 2000);
    return;
  }
  document.title = `${screen.name} — Colai VM`;
  $("name").textContent = screen.name;
  $("spec").textContent = `${screen.cpus} CPU · ${Math.round(screen.memoryMb / 1024)} GB`;
  if (screen.ownerTitle) {
    $("owner").hidden = false;
    $("owner").textContent = `Agent · ${screen.ownerTitle}`;
  }
  // Yours to drive unless an agent has it: a machine nobody owns is one you opened to use.
  if (!rfb && !connected) drive(!screen.ownerTitle);
  $("restore").hidden = !screen.savedAt;
  rfb = new RFB($("screen"), `ws://127.0.0.1:${screen.ws}`);
  rfb.scaleViewport = true;
  rfb.resizeSession = false;
  rfb.viewOnly = !driving;
  rfb.addEventListener("connect", () => {
    connected = true;
    $("dot").dataset.on = "true";
    say("");
  });
  rfb.addEventListener("disconnect", () => {
    $("dot").dataset.on = "false";
    rfb = null;
    say("Disconnected");
    // The machine may still be booting, or just restarted: look again.
    setTimeout(connect, 2000);
  });
}

$("agent").addEventListener("click", () => drive(false));
$("you").addEventListener("click", () => drive(true));
/*
 * Save point, back to it, and reset to clean. Each shuts the machine down cleanly, changes its
 * disk, and starts it again — so this window drops its connection and picks it up on the
 * machine's new port, the same way it does after any restart.
 */
async function change(said, command) {
  say(said);
  try {
    if (rfb) rfb.disconnect();
    await invoke(command, { id: id });
  } catch (trouble) {
    say(String(trouble));
    return;
  }
  say("Starting…");
}

let resetAsked = false;
$("save").addEventListener("click", () => {
  $("restore").hidden = false;
  void change("Saving…", "colai_vm_save");
});
$("restore").addEventListener("click", () => void change("Going back to the save point…", "colai_vm_restore"));
$("reset").addEventListener("click", () => {
  // Everything on the machine goes: asked once more, on the button itself.
  if (!resetAsked) {
    resetAsked = true;
    $("reset").textContent = "Reset? Click again";
    setTimeout(() => {
      resetAsked = false;
      $("reset").textContent = "Reset";
    }, 4000);
    return;
  }
  resetAsked = false;
  $("reset").textContent = "Reset";
  $("restore").hidden = true;
  void change("Resetting to clean…", "colai_vm_reset");
});

$("stop").addEventListener("click", async () => {
  say("Stopping…");
  try {
    await invoke("colai_vm_stop", { id: id });
    window.close();
  } catch (trouble) {
    say(String(trouble));
  }
});

void connect();
