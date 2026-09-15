import CopyCodeButton from "./CopyCodeButton";
import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowDownToLine,
  ArrowRight,
  Box,
  Check,
  FileArchive,
  LoaderCircle,
  LogOut,
  Plus,
  Settings2,
  UserRound,
  X,
  Play,
  ExternalLink,
  Minus,
  Maximize2,
  Home,
  Layers3,
  Terminal,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Player, { type PlayerProfile } from "./Player";
type Settings = { javaPath: string; memoryMb: number };
type Instance = {
  id: string;
  name: string;
  version: string;
  loader: string;
  status: string;
  modCount: number;
};
type Store = { settings: Settings; instances: Instance[] };
type Profile = PlayerProfile;
type Progress = {
  phase: string;
  message: string;
  current: number;
  total: number;
};
type Pack = {
  name: string;
  version: string;
  author: string;
  minecraft: string;
  loader: string;
  files: { projectID: number; fileID: number; required: boolean }[];
  overrideCount: number;
  archivePath: string;
};
type Device = {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};
type Page = "library" | "packs" | "settings" | "activity";
const desktop = isTauri();
const defaults: Settings = { javaPath: "java", memoryMb: 4096 };
async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    throw new Error(
      "Ouvre l’application Tauri pour utiliser cette fonction : npm run tauri dev.",
    );
  return invoke<T>(command, args);
}
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
function MicrosoftLogo() {
  return (
    <span className="ms-logo">
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}
export default function App() {
  const [navigation, setNavigation] = useState<{
    page: Page;
    direction: "forward" | "backward";
    animated: boolean;
  }>({ page: "library", direction: "forward", animated: false });
  const page = navigation.page;
  function setPage(next: Page) {
    const order: Page[] = ["library", "packs", "settings", "activity"];
    setNavigation((previous) =>
      previous.page === next
        ? previous
        : {
            page: next,
            direction:
              order.indexOf(next) > order.indexOf(previous.page)
                ? "forward"
                : "backward",
            animated: true,
          },
    );
  }
  const [store, setStore] = useState<Store>({
    settings: defaults,
    instances: [],
  });
  const [settings, setSettings] = useState(defaults);
  const [profile, setProfile] = useState<Profile | null>(null);
  const [selected, setSelected] = useState("");
  const [modal, setModal] = useState<"create" | "login" | null>(null);
  const [device, setDevice] = useState<Device | null>(null);
  const [loginBusy, setLoginBusy] = useState(false);
  const [versions, setVersions] = useState<{ id: string }[]>([]);
  const [version, setVersion] = useState("");
  const [name, setName] = useState("Mon monde");
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [working, setWorking] = useState(false);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(
    null,
  );
  const [progress, setProgress] = useState<Progress | null>(null);
  const [logs, setLogs] = useState<{ time: string; text: string }[]>([]);
  const [pack, setPack] = useState<Pack | null>(null);
  const [directory, setDirectory] = useState("");
  const loginGeneration = useRef(0);
  const chosen =
    store.instances.find((i) => i.id === selected) ?? store.instances[0];
  const notify = (text: string, error = false) => setNotice({ text, error });
  const log = (text: string) =>
    setLogs((l) =>
      [{ time: new Date().toLocaleTimeString("fr-FR"), text }, ...l].slice(
        0,
        200,
      ),
    );
  async function reload() {
    const s = await call<Store>("get_store");
    setStore(s);
    return s;
  }
  useEffect(() => {
    if (!desktop) return;
    let disposed = false;
    let cleanup: (() => void) | undefined;
    void listen<Progress>("launcher-progress", (e) => {
      if (disposed) return;
      setProgress(e.payload);
      if (
        e.payload.phase !== "download" ||
        e.payload.current === e.payload.total
      )
        log(e.payload.message);
    }).then((fn) => {
      if (disposed) fn();
      else cleanup = fn;
    });
    void call<Store>("get_store")
      .then((s) => {
        if (!disposed) {
          setStore(s);
          setSettings(s.settings);
        }
      })
      .catch((e) => {
        if (!disposed) notify(message(e), true);
      });
    void call<string>("data_directory").then((d) => {
      if (!disposed) setDirectory(d);
    });
    void call<Profile | null>("restore_session")
      .then((p) => {
        if (!disposed) setProfile(p);
      })
      .catch((e) => {
        if (!disposed) notify(message(e), true);
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);
  useEffect(() => {
    if (!device || modal !== "login") return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const p = await call<Profile | null>("poll_login");
        if (stopped) return;
        if (p) {
          setProfile(p);
          setModal(null);
          setDevice(null);
          notify(`Connecté en tant que ${p.name}.`);
          log("Connexion Microsoft réussie.");
        } else timer = setTimeout(poll, device.interval * 1000);
      } catch (e) {
        if (!stopped) {
          setDevice(null);
          notify(message(e), true);
        }
      }
    };
    timer = setTimeout(poll, device.interval * 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [device, modal]);
  async function startLogin() {
    const generation = ++loginGeneration.current;
    setModal("login");
    setLoginBusy(true);
    try {
      const d = await call<Device>("begin_login");
      if (generation === loginGeneration.current) setDevice(d);
      else await call("cancel_login");
    } catch (e) {
      notify(message(e), true);
    } finally {
      setLoginBusy(false);
    }
  }
  async function closeModal() {
    loginGeneration.current++;
    setModal(null);
    setDevice(null);
    if (desktop) await call("cancel_login").catch(() => {});
  }
  async function createModal() {
    setModal("create");
    if (versions.length) return;
    setWorking(true);
    try {
      const v = await call<{ id: string }[]>("list_versions");
      setVersions(v);
      setVersion(v[0]?.id ?? "");
    } catch (e) {
      notify(message(e), true);
    } finally {
      setWorking(false);
    }
  }
  async function create() {
    setWorking(true);
    try {
      const i = await call<Instance>("create_instance", { name, version });
      await reload();
      setSelected(i.id);
      setModal(null);
      notify("Instance créée. Le premier lancement téléchargera Minecraft.");
      log(`Instance créée : ${i.name} (${i.version}).`);
    } catch (e) {
      notify(message(e), true);
    } finally {
      setWorking(false);
    }
  }
  async function launch() {
    if (!chosen) {
      await createModal();
      return;
    }
    if (!profile) {
      await startLogin();
      return;
    }
    setBusy(true);
    setProgress({
      phase: "prepare",
      message: "Préparation du lancement…",
      current: 0,
      total: 0,
    });
    try {
      await call("launch_instance", { id: chosen.id });
      await reload();
    } catch (e) {
      notify(message(e), true);
      log(message(e));
    } finally {
      setBusy(false);
    }
  }
  async function installPack() {
    if (!pack || busy) return;
    setBusy(true);
    setInstalling(true);
    setNotice(null);
    setProgress({ phase: "prepare", message: "Préparation du pack…", current: 0, total: 0 });
    try {
      const instance = await call<Instance>("install_modpack", { path: pack.archivePath });
      await reload();
      setSelected(instance.id);
      setPack(null);
      setPage("library");
      notify(`${instance.name} installé.`);
    } catch (e) {
      notify(message(e), true);
    } finally {
      setInstalling(false);
      setBusy(false);
    }
  }
  async function inspect() {
    setWorking(true);
    try {
      if (!desktop)
        throw new Error(
          "L’import de ZIP est disponible dans l’application Tauri.",
        );
      const path = await open({
        multiple: false,
        filters: [{ name: "Modpack CurseForge", extensions: ["zip"] }],
      });
      if (path) {
        const p = await call<Pack>("inspect_modpack", { path });
        setPack(p);
        setPage("packs");
        log(`Archive analysée : ${p.name}, ${p.files.length} mods.`);
      }
    } catch (e) {
      notify(message(e), true);
    } finally {
      setWorking(false);
    }
  }
  async function save() {
    setWorking(true);
    try {
      await call("save_settings", { settings });
      await reload();
      notify("Paramètres enregistrés.");
    } catch (e) {
      notify(message(e), true);
    } finally {
      setWorking(false);
    }
  }
  async function windowAction(action: "close" | "minimize" | "toggleMaximize") {
    if (!desktop) return;
    try {
      await getCurrentWindow()[action]();
    } catch (e) {
      notify(message(e), true);
    }
  }
  return (
    <div className="app-shell">
      <header className="titlebar" data-tauri-drag-region>
        <span className="brand" data-tauri-drag-region>
          Hx<span>launcher</span>
        </span>
        <div className="window-controls">
          <button
            aria-label="Réduire"
            onClick={() => void windowAction("minimize")}
          >
            <Minus size={14} />
          </button>
          <button
            aria-label="Agrandir"
            onClick={() => void windowAction("toggleMaximize")}
          >
            <Maximize2 size={12} />
          </button>
          <button
            aria-label="Fermer la fenêtre"
            onClick={() => void windowAction("close")}
          >
            <X size={15} />
          </button>
        </div>
      </header>
      <div className="toolbar">
        <nav aria-label="Navigation principale">
          {(
            [
              { id: "library", label: "Accueil", icon: Home },
              { id: "packs", label: "Modpacks", icon: Layers3 },
              { id: "settings", label: "Réglages", icon: Settings2 },
            ] as const
          ).map((n) => (
            <button
              className={page === n.id ? "active" : ""}
              aria-current={page === n.id ? "page" : undefined}
              key={n.id}
              onClick={() => setPage(n.id)}
            >
              <n.icon size={16} />
              {n.label}
            </button>
          ))}
        </nav>
        <button
          className="account"
          disabled={busy || loginBusy}
          onClick={() => (profile ? setPage("settings") : void startLogin())}
        >
          <UserRound size={16} />
          {profile?.name ?? "Connexion Microsoft"}
          {profile && <span className="online-dot" />}
        </button>
      </div>
      {notice && !modal && (
        <div className={`notice ${notice.error ? "error" : ""}`} role="status">
          <span>{notice.text}</span>
          <button
            aria-label="Fermer la notification"
            onClick={() => setNotice(null)}
          >
            <X size={15} />
          </button>
        </div>
      )}
      <main
        key={page}
        className={navigation.animated ? "tab-slide" : undefined}
        data-direction={navigation.direction}
      >
        {page === "library" && (
          <section className="home">
            <div className="home-copy">
              <div className="launch-panel">
                <div className="instance-picker">
                  <Box size={21} />
                  <div>
                    {chosen ? (
                      <select
                        aria-label="Instance à lancer"
                        value={chosen.id}
                        disabled={busy}
                        onChange={(e) => setSelected(e.target.value)}
                      >
                        {store.instances.map((i) => (
                          <option value={i.id} key={i.id}>
                            {i.name} · {i.version}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <strong>Aucune instance</strong>
                    )}
                  </div>
                  <button
                    className="icon-button"
                    title="Nouvelle instance"
                    aria-label="Nouvelle instance"
                    disabled={busy}
                    onClick={() => void createModal()}
                  >
                    <Plus size={19} />
                  </button>
                </div>
                <button
                  className="button primary play-button"
                  disabled={busy}
                  onClick={() => void launch()}
                >
                  {busy ? (
                    <LoaderCircle size={18} className="spin" />
                  ) : (
                    <Play size={17} fill="currentColor" />
                  )}
                  {busy
                    ? progress?.phase === "running"
                      ? "Jeu en cours"
                      : "Préparation…"
                    : chosen
                      ? "Jouer"
                      : "Créer mon monde"}
                  {!busy && <ArrowRight size={17} />}
                </button>
                {busy && progress && (
                  <div className="inline-progress">
                    <span>{progress.message}</span>
                    {progress.total > 0 && (
                      <progress max={progress.total} value={progress.current} />
                    )}
                  </div>
                )}
              </div>
              <button
                className="subtle-link"
                disabled={working}
                onClick={() => setPage("packs")}
              >
                <FileArchive size={14} />
                Importer un modpack
                <ArrowRight size={13} />
              </button>
            </div>
            <Player profile={profile} />
          </section>
        )}
        {page === "packs" && (
          <section className="simple-page">
            <div className="page-heading">
              <h1>Modpacks.</h1>
            </div>
            <div className="page-content">
              <div className="glass import-panel">
                <FileArchive size={38} strokeWidth={1.2} />
                <h2>Importer un modpack</h2>
                <button
                  className="button primary"
                  disabled={working || busy}
                  onClick={() => void inspect()}
                >
                  {working ? (
                    <LoaderCircle className="spin" size={17} />
                  ) : (
                    <Plus size={17} />
                  )}
                  Choisir un ZIP
                </button>
                <small>Export CurseForge · .zip</small>
              </div>
              {pack && (
                <div className="glass pack-result">
                  <h2>{pack.name}</h2>
                  <p>
                    {pack.minecraft} · {pack.loader} · {pack.files.length} mods
                    · {pack.overrideCount} configurations
                  </p>
                  <button className="button primary" disabled={busy || working} onClick={() => void installPack()}>
                    {installing ? <LoaderCircle className="spin" size={17} /> : <ArrowDownToLine size={17} />}
                    {installing ? "Installation…" : "Installer"}
                  </button>
                  {installing && progress && <div className="inline-progress" role="status">
                    <span>{progress.message}</span>
                    <progress max={progress.total || undefined} value={progress.total ? progress.current : undefined} />
                  </div>}
                </div>
              )}
            </div>
          </section>
        )}
        {page === "settings" && (
          <section className="simple-page settings-page">
            <div className="page-heading">
              <h1>Réglages.</h1>
            </div>
            <div className="glass settings-card">
              <div className="section-title">
                <h2>Compte Microsoft</h2>
                {profile && (
                  <button
                    className="subtle-link"
                    disabled={busy}
                    onClick={() =>
                      void call("logout")
                        .then(() => setProfile(null))
                        .catch((e) => notify(message(e), true))
                    }
                  >
                    <LogOut size={14} />
                    Déconnexion
                  </button>
                )}
              </div>
              <div className="settings-grid">
                <label className="field">
                  Exécutable Java
                  <div className="input-button">
                    <input
                      value={settings.javaPath}
                      disabled={busy}
                      onChange={(e) =>
                        setSettings({ ...settings, javaPath: e.target.value })
                      }
                    />
                    <button
                      disabled={busy || working}
                      onClick={() => {
                        setWorking(true);
                        void call<string>("check_java", {
                          path: settings.javaPath,
                        })
                          .then((v) => notify(v))
                          .catch((e) => notify(message(e), true))
                          .finally(() => setWorking(false));
                      }}
                    >
                      Vérifier
                    </button>
                  </div>
                </label>
                <label className="field">
                  Mémoire <strong>{settings.memoryMb / 1024} Go</strong>
                  <input
                    type="range"
                    min={1024}
                    max={16384}
                    step={512}
                    value={settings.memoryMb}
                    disabled={busy}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        memoryMb: Number(e.target.value),
                      })
                    }
                  />
                </label>
              </div>
              <div className="settings-bottom">
                <button
                  className="button primary"
                  disabled={busy || working}
                  onClick={() => void save()}
                >
                  <Check size={16} />
                  Enregistrer
                </button>
                <button
                  className="button secondary"
                  disabled={busy || loginBusy}
                  onClick={() => void startLogin()}
                >
                  <MicrosoftLogo />
                  {profile ? "Changer de compte" : "Se connecter"}
                </button>
              </div>
              <details className="storage">
                <summary>Dossier des instances</summary>
                <p>{directory || "Disponible dans l’application native"}</p>
              </details>
            </div>
          </section>
        )}
        {page === "activity" && (
          <section className="simple-page">
            <div className="page-heading">
              <h1>Activité.</h1>
            </div>
            <div className="glass logs">
              {logs.length ? (
                logs.map((l, i) => (
                  <p key={i}>
                    <time>{l.time}</time>
                    {l.text}
                  </p>
                ))
              ) : (
                <p>Rien à signaler pour le moment.</p>
              )}
            </div>
          </section>
        )}
      </main>
      <footer>
        <span>
          {desktop ? "Minecraft Java Edition" : "Aperçu navigateur"}
          <i />
          v0.1.0
        </span>
        <button
          onClick={() => setPage(page === "activity" ? "library" : "activity")}
        >
          <Terminal size={13} />
          Activité
        </button>
      </footer>
      {modal && (
        <div
          className="modal-backdrop"
          onClick={() => {
            if (!working) void closeModal();
          }}
        >
          <section
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="modal-title"
            onClick={(e) => e.stopPropagation()}
          >
            {notice?.error && (
              <div className="notice error" role="alert">
                {notice.text}
              </div>
            )}
            <button
              className="modal-close"
              disabled={working}
              onClick={() => void closeModal()}
              aria-label="Fermer"
            >
              <X size={20} />
            </button>
            {modal === "create" ? (
              <>
                <div className="large-icon">
                  <Box size={30} />
                </div>
                <h2 id="modal-title">Nouvelle instance.</h2>
                <label className="field">
                  Nom de l’instance
                  <input
                    autoFocus
                    value={name}
                    maxLength={80}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="Mon monde"
                  />
                </label>
                <label className="field">
                  Version Minecraft
                  <select
                    value={version}
                    onChange={(e) => setVersion(e.target.value)}
                    disabled={working}
                  >
                    {!versions.length && (
                      <option>
                        {working
                          ? "Chargement des versions…"
                          : "Versions indisponibles"}
                      </option>
                    )}
                    {versions.map((v) => (
                      <option key={v.id}>{v.id}</option>
                    ))}
                  </select>
                  <small>
                    Versions stables récentes, récupérées directement chez
                    Mojang.
                  </small>
                </label>
                <div className="modal-note">
                  <ArrowDownToLine size={17} />
                  Minecraft sera téléchargé au premier lancement.
                </div>
                <button
                  className="button primary full"
                  disabled={working || !version || !name.trim()}
                  onClick={() => void create()}
                >
                  {working ? (
                    <LoaderCircle className="spin" size={16} />
                  ) : (
                    <Plus size={16} />
                  )}
                  Créer l’instance
                </button>
              </>
            ) : (
              <>
                <div className="large-icon">
                  <MicrosoftLogo />
                </div>
                <h2 id="modal-title">Ton compte, ton aventure.</h2>
                <p>
                  Connecte-toi sur Microsoft avec ce code. Ton mot de passe
                  reste chez Microsoft.
                </p>
                {loginBusy ? (
                  <div className="login-wait">
                    <LoaderCircle className="spin" />
                    Préparation de la connexion…
                  </div>
                ) : device ? (
                  <>
                    <div className="device-code">
                      {device.userCode}
                      <CopyCodeButton
                        key={device.userCode}
                        code={device.userCode}
                      />
                    </div>
                    <button
                      className="button primary full"
                      onClick={() =>
                        void openUrl(device.verificationUri).catch((e) =>
                          notify(message(e), true),
                        )
                      }
                    >
                      Ouvrir Microsoft <ExternalLink size={16} />
                    </button>
                    <p className="verification-url">{device.verificationUri}</p>
                    <div className="login-wait">
                      <LoaderCircle className="spin" size={16} />
                      En attente de ta connexion…
                    </div>
                  </>
                ) : (
                  <button
                    className="button primary full"
                    onClick={() => void startLogin()}
                  >
                    Réessayer
                  </button>
                )}
              </>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
