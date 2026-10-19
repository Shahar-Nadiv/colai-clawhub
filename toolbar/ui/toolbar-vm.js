// Virtual machines: the VM key's popup.
//
// A shelf like the artifacts one: "+ New VM" first, then each machine with a live picture of its
// screen, its name, whether it runs, and whose it is. Open shows its screen in a window of its
// own (vm.html). The machines run on this computer, on Colai's own engine (colai_vm.rs). The
// first time the popup opens it gets everything ready by itself — nothing for anyone to install,
// no account, no key — and says how far it has got.

function openVms() {
  state.vmNew = null;
  state.vmRemoving = null;
  state.vmTrouble = "";
  flyout("vms");
  if (state.open === "vms") void loadVms();
}

let vmTicker = 0;

async function loadVms() {
  try {
    state.vmSetup = await invoke("colai_vm_setup");
  } catch (trouble) {
    state.vmTrouble = String(trouble);
  }
  // Nobody sets anything up: the first look starts it.
  if (state.vmSetup && state.vmSetup.state === "missing") {
    try {
      await invoke("colai_vm_build");
      state.vmSetup = await invoke("colai_vm_setup");
    } catch (trouble) {
      state.vmTrouble = String(trouble);
    }
  }
  // A Windows VM asked for before Windows was ready: made as soon as it is.
  const pending = state.vmPending;
  if (pending && state.vmSetup && state.vmSetup.windows) {
    const windows = state.vmSetup.windows;
    if (windows.state === "ready") {
      state.vmPending = null;
      await makeVm(pending);
    } else if (windows.state === "failed") {
      state.vmPending = null;
      state.vmBusy = "";
      state.vmTrouble = windows.said;
    } else {
      state.vmBusy = windows.said || "Getting Windows ready";
    }
  }
  if (state.vmSetup && state.vmSetup.state === "ready") {
    try {
      state.vms = (await invoke("colai_vm_list")) || [];
    } catch (trouble) {
      state.vmTrouble = String(trouble);
    }
    for (const vm of state.vms) {
      if (!vm.running) continue;
      invoke("colai_vm_thumb", { id: vm.id })
        .then((shot) => {
          state.vmThumbs[vm.id] = shot;
          render();
        })
        .catch(() => {});
    }
  }
  render();
  // Kept fresh while it is open: a machine boots, setup moves on, a screen changes.
  clearTimeout(vmTicker);
  if (state.open === "vms") vmTicker = setTimeout(() => void loadVms(), 3000);
}

function vmsRunning() {
  return (state.vms || []).filter((vm) => vm.running).length;
}

function vmButton(className, said, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.textContent = said;
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    onClick();
  });
  return button;
}

function vmFail(trouble) {
  state.vmBusy = "";
  state.vmTrouble = trouble && trouble.message ? trouble.message : String(trouble);
  render();
}

function vmNote(text) {
  const said = document.createElement("p");
  said.className = "art-note";
  said.textContent = text;
  return said;
}

function drawVms() {
  if (!el.flyVm) return;
  el.flyVm.hidden = state.open !== "vms";
  if (buttons.vm) {
    const running = vmsRunning();
    buttons.vm.dataset.running = running ? String(running) : "";
  }
  if (state.open !== "vms") return;

  const head = document.createElement("div");
  head.className = "kit-head art-head";
  const title = document.createElement("span");
  title.textContent = state.vmNew ? "New VM" : "Virtual machines";
  head.append(title);

  const bits = [head];
  const setup = state.vmSetup;
  if (!setup) {
    bits.push(vmNote("Looking…"));
  } else if (setup.state !== "ready") {
    bits.push(vmGettingReady(setup));
  } else if (state.vmNew) {
    bits.push(...vmForm());
  } else {
    bits.push(vmGrid());
  }
  if (state.vmBusy) bits.push(vmNote(state.vmBusy));
  if (state.vmTrouble) {
    const said = vmNote(state.vmTrouble);
    said.classList.add("art-trouble");
    bits.push(said);
  }
  el.flyVm.replaceChildren(...bits);
}

/** Getting ready, the first time: one line of where it is, or what stopped it. */
function vmGettingReady(setup) {
  const row = document.createElement("div");
  row.className = "vm-step";
  row.dataset.done = "false";
  const said = document.createElement("span");
  said.className = "vm-step-said";
  said.textContent = setup.state === "failed" ? setup.said : setup.said || "Getting VMs ready";
  row.append(said);
  if (setup.state === "failed") {
    row.append(
      vmButton("vm-copy", "Try again", () => {
        state.vmTrouble = "";
        invoke("colai_vm_build").then(() => void loadVms(), vmFail);
      }),
    );
  }
  return row;
}

function vmGrid() {
  const grid = document.createElement("div");
  grid.className = "art-grid";

  const add = document.createElement("button");
  add.type = "button";
  add.className = "art-card kit-add";
  const plus = document.createElement("span");
  plus.className = "art-thumb art-new";
  plus.textContent = "+";
  const addMeta = document.createElement("span");
  addMeta.className = "art-meta";
  const addSaid = document.createElement("span");
  addSaid.className = "kit-name art-title";
  addSaid.textContent = "New VM";
  addMeta.append(addSaid);
  add.append(plus, addMeta);
  add.addEventListener("click", () => {
    state.vmNew = { name: "", os: "ubuntu", size: "medium", owner: state.receiving.id || "", ownerTitle: state.receiving.name || "" };
    state.vmTrouble = "";
    render();
  });
  grid.append(add);

  for (const vm of state.vms || []) grid.append(vmCard(vm));
  return grid;
}

function vmCard(vm) {
  const card = document.createElement("div");
  card.className = "art-card vm-card";
  card.dataset.id = vm.id;

  const screen = document.createElement("span");
  screen.className = "art-thumb vm-screen";
  const shot = vm.running ? state.vmThumbs[vm.id] : "";
  if (shot) {
    const img = document.createElement("img");
    img.className = "vm-shot";
    img.alt = "";
    img.src = shot;
    screen.append(img);
  } else {
    screen.textContent = vm.running ? "Starting…" : "Stopped";
  }

  const meta = document.createElement("span");
  meta.className = "art-meta";
  const name = document.createElement("span");
  name.className = "kit-name art-title vm-name";
  const dot = document.createElement("span");
  dot.className = "vm-dot";
  dot.dataset.on = String(Boolean(vm.running));
  name.append(dot, document.createTextNode(vm.name));
  const sub = document.createElement("span");
  sub.className = "art-sub";
  sub.textContent = [vm.os === "windows" ? "Windows" : "Ubuntu", vm.ownerTitle || "Free", `${vm.cpus} CPU · ${Math.round(vm.memoryMb / 1024)} GB`].join(" · ");
  meta.append(name, sub);

  const acts = document.createElement("span");
  acts.className = "vm-acts";
  const busy = (said, call) => {
    state.vmTrouble = "";
    state.vmBusy = said;
    render();
    call.then(() => {
      state.vmBusy = "";
      void loadVms();
    }, vmFail);
  };
  if (state.vmRemoving === vm.id) {
    acts.append(
      vmButton("kit-save", "Delete", () => {
        state.vmRemoving = null;
        busy("Deleting…", invoke("colai_vm_delete", { id: vm.id }));
      }),
      vmButton("kit-cancel", "Keep", () => {
        state.vmRemoving = null;
        render();
      }),
    );
  } else {
    // Whose it is: a free machine can be given to the chat being sent to; a taken one can be
    // sent to — marks then go to its agent, which works inside it.
    const here = state.receiving.id;
    if (vm.owner && vm.owner !== here) {
      acts.append(
        vmButton("kit-cancel", "Send here", () => {
          receive(vm.owner, vm.ownerTitle || vm.name);
          state.open = null;
          render();
        }),
      );
    } else if (!vm.owner && here) {
      acts.append(
        vmButton("kit-cancel", "Give to this chat", () =>
          busy("Giving it to this chat…", invoke("colai_vm_bind", { id: vm.id, owner: here, ownerTitle: state.receiving.name || null })),
        ),
      );
    }
    acts.append(
      vmButton("kit-save", "Open", () => busy(`Opening ${vm.name}…`, invoke("colai_vm_open", { id: vm.id }))),
      vm.running
        ? vmButton("kit-cancel", "Stop", () => busy("Stopping…", invoke("colai_vm_stop", { id: vm.id })))
        : vmButton("kit-cancel", "Start", () => busy("Starting…", invoke("colai_vm_start", { id: vm.id }))),
      vmButton("kit-cancel", "Delete", () => {
        state.vmRemoving = vm.id;
        render();
      }),
    );
  }
  card.append(screen, meta, acts);
  return card;
}

const VM_SIZES = [
  { id: "small", label: "Small", said: "2 CPU · 2 GB" },
  { id: "medium", label: "Medium", said: "2 CPU · 4 GB" },
  { id: "large", label: "Large", said: "4 CPU · 8 GB" },
];

/** The systems a VM can be. Windows is Microsoft's free evaluation, so its terms are a link away. */
const VM_SYSTEMS = [
  { id: "ubuntu", label: "Ubuntu", said: "24.04 · desktop" },
  { id: "windows", label: "Windows", said: "Server 2025 · evaluation, 180 days" },
];
const WINDOWS_TERMS = "https://www.microsoft.com/en-us/evalcenter/evaluate-windows-server-2025";

/** A row of choices, drawn like the sizes. */
function vmChoiceRow(options, chosen, onPick) {
  const row = document.createElement("div");
  row.className = "vm-sizes";
  for (const option of options) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "vm-size";
    button.dataset.on = String(chosen === option.id);
    const label = document.createElement("span");
    label.className = "vm-size-label";
    label.textContent = option.label;
    const said = document.createElement("span");
    said.className = "art-sub";
    said.textContent = option.said;
    button.append(label, said);
    button.addEventListener("click", () => onPick(option.id));
    row.append(button);
  }
  return row;
}

function vmForm() {
  const form = state.vmNew;
  const nameLabel = document.createElement("label");
  nameLabel.className = "kit-label";
  nameLabel.textContent = "Name";
  const name = document.createElement("input");
  name.className = "kit-input";
  name.dataset.field = "vm-name";
  name.maxLength = 40;
  name.placeholder = "checkout-redesign";
  name.value = form.name;
  name.addEventListener("input", () => {
    form.name = name.value;
  });
  nameLabel.append(name);

  const systemLabel = document.createElement("div");
  systemLabel.className = "kit-label";
  systemLabel.textContent = "System";
  const systems = vmChoiceRow(VM_SYSTEMS, form.os, (os) => {
    form.os = os;
    // A Windows desktop needs room: it starts at Medium.
    if (os === "windows" && form.size === "small") form.size = "medium";
    render();
  });
  const bits = [nameLabel, systemLabel, systems];
  if (form.os === "windows") {
    const terms = document.createElement("button");
    terms.type = "button";
    terms.className = "vm-terms";
    terms.textContent = "Microsoft evaluation terms";
    terms.addEventListener("click", () => {
      void invoke("plugin:opener|open_url", { url: WINDOWS_TERMS }).catch(() => {});
    });
    bits.push(terms);
  }

  const sizeLabel = document.createElement("div");
  sizeLabel.className = "kit-label";
  sizeLabel.textContent = "Size";
  const sizes = vmChoiceRow(
    VM_SIZES.filter((size) => form.os !== "windows" || size.id !== "small"),
    form.size,
    (size) => {
      form.size = size;
      render();
    },
  );

  const agent = document.createElement("div");
  agent.className = "kit-label";
  agent.textContent = "Agent";
  const who = document.createElement("div");
  who.className = "vm-agent";
  who.textContent = form.ownerTitle || "None";

  const row = document.createElement("div");
  row.className = "kit-actions";
  row.append(
    vmButton("kit-cancel", "Cancel", () => {
      state.vmNew = null;
      render();
    }),
    vmButton("kit-save", "Create VM", () => void createVm()),
  );
  return [...bits, sizeLabel, sizes, agent, who, row];
}

async function createVm() {
  const form = state.vmNew;
  if (!form || state.vmBusy) return;
  state.vmTrouble = "";
  const windows = state.vmSetup && state.vmSetup.windows;
  // Windows is built the first time it is asked for; the VM is made once it is ready.
  if (form.os === "windows" && (!windows || windows.state !== "ready")) {
    if (!String(form.name || "").trim()) {
      vmFail("Give the VM a name.");
      return;
    }
    try {
      await invoke("colai_vm_build", { os: "windows" });
    } catch (trouble) {
      vmFail(trouble);
      return;
    }
    state.vmPending = { ...form };
    state.vmNew = null;
    state.vmBusy = "Getting Windows ready";
    void loadVms();
    return;
  }
  state.vmNew = null;
  await makeVm(form);
  void loadVms();
}

async function makeVm(form) {
  try {
    await invoke("colai_vm_create", {
      name: form.name,
      size: form.size,
      os: form.os || "ubuntu",
      owner: form.owner || null,
      ownerTitle: form.ownerTitle || null,
    });
    state.vmBusy = "";
  } catch (trouble) {
    state.vmNew = form;
    vmFail(trouble);
  }
}
