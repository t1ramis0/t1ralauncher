const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (s) => document.querySelector(s);

// Любая ошибка интерфейса попадает в crash.log рядом с данными лаунчера
invoke("log_js", { msg: "интерфейс загружен" }).catch(() => {});
window.addEventListener("error", (e) => {
  invoke("log_js", { msg: `${e.message} (${e.filename}:${e.lineno})` }).catch(() => {});
});
window.addEventListener("unhandledrejection", (e) => {
  invoke("log_js", { msg: "unhandled: " + String(e.reason) }).catch(() => {});
});

const MAX_LOG = 3000;
const state = {
  instances: [],
  selected: null,
  running: new Set(),
  logs: {},
  versions: null,
  busy: false,
};

// ---------- утилиты ----------
function showError(msg) {
  const b = $("#banner");
  b.textContent = String(msg);
  b.classList.remove("hidden");
}
function clearError() { $("#banner").classList.add("hidden"); }
function current() { return state.instances.find((i) => i.id === state.selected) || null; }
function loaderLabel(i) {
  const l = { vanilla: "Vanilla", fabric: "Fabric", quilt: "Quilt" }[i.loader] || i.loader;
  return `${l} · Minecraft ${i.mc_version}`;
}
function setProgress(stage, done, total) {
  const p = $("#progress");
  if (!stage) { p.classList.add("hidden"); return; }
  p.classList.remove("hidden");
  $("#bar").style.width = total ? `${Math.round((done / total) * 100)}%` : "0%";
  $("#ptext").textContent = total > 1 ? `${stage}: ${done} / ${total}` : stage;
}

// ---------- отрисовка ----------
function renderList() {
  const list = $("#list");
  list.replaceChildren();
  for (const i of state.instances) {
    const b = document.createElement("button");
    b.className = "inst" + (i.id === state.selected ? " active" : "");
    const t = document.createElement("div");
    t.className = "t";
    if (state.running.has(i.id)) {
      const d = document.createElement("span");
      d.className = "dot";
      d.title = "Запущена";
      t.append(d);
    }
    t.append(document.createTextNode(i.name));
    const s = document.createElement("div");
    s.className = "s";
    s.textContent = loaderLabel(i);
    b.append(t, s);
    b.onclick = () => { state.selected = i.id; clearError(); render(); };
    list.append(b);
  }
}

function renderMain() {
  const inst = current();
  $("#empty").classList.toggle("hidden", state.instances.length > 0);
  $("#view").classList.toggle("hidden", !inst);
  if (!inst) return;
  $("#v-name").textContent = inst.name;
  $("#v-sub").textContent = loaderLabel(inst);
  $("#v-ram").value = inst.ram_mb ?? "";
  const run = state.running.has(inst.id);
  $("#btn-launch").classList.toggle("hidden", run);
  $("#btn-launch").disabled = state.busy;
  $("#btn-stop").classList.toggle("hidden", !run);
  $("#btn-delete").disabled = run;
  const c = $("#console");
  c.textContent = (state.logs[inst.id] || []).join("\n");
  c.scrollTop = c.scrollHeight;
}

function render() { renderList(); renderMain(); }

async function refresh(select) {
  state.instances = await invoke("list_instances");
  if (select) state.selected = select;
  if (!current()) state.selected = state.instances[0]?.id ?? null;
  render();
}

function appendLog(id, line) {
  const arr = (state.logs[id] ||= []);
  arr.push(line);
  if (arr.length > MAX_LOG) arr.splice(0, arr.length - MAX_LOG);
  if (id === state.selected) {
    const c = $("#console");
    const stick = c.scrollTop + c.clientHeight >= c.scrollHeight - 40;
    c.append(document.createTextNode((c.textContent ? "\n" : "") + line));
    if (stick) c.scrollTop = c.scrollHeight;
  }
}

// ---------- события бэкенда ----------
listen("progress", (e) => setProgress(e.payload.stage, e.payload.done, e.payload.total));
listen("log", (e) => appendLog(e.payload.id, e.payload.line));
listen("started", (e) => { state.running.add(e.payload.id); render(); });
listen("exited", (e) => {
  state.running.delete(e.payload.id);
  appendLog(e.payload.id, `[лаунчер] Игра завершилась, код выхода: ${e.payload.code ?? "—"}`);
  render();
});

// ---------- действия ----------
$("#btn-launch").onclick = async () => {
  const inst = current();
  if (!inst) return;
  clearError();
  state.logs[inst.id] = [];
  state.busy = true; render();
  try {
    await invoke("launch_instance", { id: inst.id });
  } catch (err) {
    showError(err);
  } finally {
    state.busy = false;
    setProgress(null);
    await refresh();
  }
};
$("#btn-stop").onclick = () => { const i = current(); if (i) invoke("stop_instance", { id: i.id }); };
$("#btn-folder").onclick = async () => {
  const i = current();
  if (!i) return;
  try { await invoke("open_instance_folder", { id: i.id }); } catch (err) { showError(err); }
};
$("#btn-delete").onclick = async () => {
  const i = current();
  if (!i) return;
  if (!confirm(`Удалить сборку «${i.name}» вместе с мирами и модами?`)) return;
  try {
    await invoke("delete_instance", { id: i.id });
    delete state.logs[i.id];
    state.selected = null;
    await refresh();
  } catch (err) { showError(err); }
};
$("#v-ram").onchange = async () => {
  const i = current();
  if (!i) return;
  const v = $("#v-ram").value.trim();
  try {
    await invoke("set_instance_ram", { id: i.id, ramMb: v ? parseInt(v, 10) : null });
    await refresh();
  } catch (err) { showError(err); }
};

// ---------- новая сборка ----------
function fillVersions() {
  const all = $("#n-snap").checked;
  const sel = $("#n-version");
  sel.replaceChildren();
  for (const v of state.versions || []) {
    if (!all && v.type !== "release") continue;
    const o = document.createElement("option");
    o.value = v.id;
    o.textContent = v.type === "release" ? v.id : `${v.id} (${v.type})`;
    sel.append(o);
  }
}
async function openNew() {
  clearError();
  $("#n-name").value = "";
  $("#dlg-new").showModal();
  if (!state.versions) {
    $("#n-version").replaceChildren(new Option("Загрузка списка версий…", ""));
    try {
      state.versions = await invoke("remote_versions");
    } catch (err) {
      $("#dlg-new").close();
      showError("Не удалось получить список версий: " + err + "\nПроверь интернет или включи зеркало в настройках.");
      return;
    }
  }
  fillVersions();
}
$("#btn-new").onclick = openNew;
$("#btn-new-2").onclick = openNew;
$("#n-snap").onchange = fillVersions;
$("#n-cancel").onclick = () => $("#dlg-new").close();
$("#n-ok").onclick = async () => {
  const name = $("#n-name").value.trim();
  const mcVersion = $("#n-version").value;
  const loader = $("#n-loader").value;
  if (!name || !mcVersion) { showError("Введи название и выбери версию."); return; }
  $("#dlg-new").close();
  clearError();
  state.busy = true; render();
  try {
    const inst = await invoke("create_instance", { name, mcVersion, loader });
    await refresh(inst.id);
  } catch (err) {
    showError(err);
  } finally {
    state.busy = false;
    setProgress(null);
    render();
  }
};

// ---------- настройки ----------
$("#btn-settings").onclick = async () => {
  const s = await invoke("get_settings");
  $("#s-name").value = s.username;
  $("#s-ram").value = s.max_ram_mb;
  $("#s-java").value = s.java_path;
  $("#s-args").value = s.java_args;
  $("#s-mirror").checked = s.use_mirror;
  $("#dlg-settings").showModal();
};
$("#s-cancel").onclick = () => $("#dlg-settings").close();
$("#s-ok").onclick = async () => {
  try {
    await invoke("save_settings", {
      s: {
        username: $("#s-name").value,
        max_ram_mb: parseInt($("#s-ram").value, 10) || 4096,
        java_path: $("#s-java").value,
        java_args: $("#s-args").value,
        use_mirror: $("#s-mirror").checked,
      },
    });
    state.versions = null;
    $("#dlg-settings").close();
  } catch (err) { showError(err); }
};

refresh();
