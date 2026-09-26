import CopyCodeButton from "./components/CopyCodeButton";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowDownToLine,
  ArrowRight,
  Box,
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
  Moon,
  Sun,
  Home,
  Layers3,
  Terminal,
  Trash2,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import Player, { type PlayerProfile } from "./Player";
type Profile = PlayerProfile;
type MemoryInfo = {
  totalMb: number;
};
import type {
  Device,
  Instance,
  LogEntry,
  Pack,
  Progress,
  Settings,
  Store,
} from "./types";
import { call, desktop, getErrorMessage } from "./lib/tauri";
import { usePageNavigation } from "./hooks/usePageNavigation";
import { useToast } from "./hooks/useToast";

const defaults: Settings = { memoryMb: 4096, storageDirectory: "" };
const previewProfile: Profile = {
  id: "preview-hydroxios",
  name: "Hydroxios",
  skins: [{ url: "/skins/hydro.png", variant: "CLASSIC", state: "ACTIVE" }],
};
const message = getErrorMessage;
const navigationItems = [
  { id: "library", label: "Accueil", icon: Home },
  { id: "packs", label: "Instances", icon: Layers3 },
] as const;
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
function AccountAvatar({ name }: { name: string }) {
  const [failed, setFailed] = useState(false);
  return failed ? (
    <UserRound size={16} />
  ) : (
    <img
      className="account-avatar"
      src={`https://mc-heads.net/avatar/${encodeURIComponent(name)}/32`}
      alt={`Avatar de ${name}`}
      onError={() => setFailed(true)}
    />
  );
}
function InstanceIcon({ instance }: { instance: Instance }) {
  return instance.iconPath ? (
    <img className="instance-image" src={instance.iconPath} alt="" />
  ) : (
    <Box size={20} />
  );
}
export default function App() {
  const [darkMode, setDarkMode] = useState(() => {
    const saved = localStorage.getItem("hx-theme");
    return saved
      ? saved === "dark"
      : window.matchMedia("(prefers-color-scheme: dark)").matches;
  });
  const { page, direction, animated, setPage } = usePageNavigation();
  const [store, setStore] = useState<Store>({
    settings: defaults,
    instances: [],
  });
  const [settings, setSettings] = useState(defaults);
  const [memoryInfo, setMemoryInfo] = useState<MemoryInfo | null>(null);
  const [profile, setProfile] = useState<Profile | null>(
    desktop ? null : previewProfile,
  );
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
  const [progress, setProgress] = useState<Progress | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [pack, setPack] = useState<Pack | null>(null);
  const [directory, setDirectory] = useState("");
  const loginGeneration = useRef(0);
  const [settingsReady, setSettingsReady] = useState(!desktop);
  const [pendingSettings, setPendingSettings] = useState<Settings | null>(null);
  const [restoringSession, setRestoringSession] = useState(desktop);
  const { toast, showToast, dismissToast } = useToast();
  const chosen =
    store.instances.find((i) => i.id === selected) ?? store.instances[0];
  const modpacks = store.instances;
  const notify = showToast;
  const log = (text: string) => {
    const entry = { time: new Date().toLocaleTimeString("fr-FR"), text };
    setLogs((entries) => [entry, ...entries].slice(0, 200));
  };
  function updateSettings(next: Settings) {
    if (
      !settingsReady ||
      (next.memoryMb === settings.memoryMb &&
        next.storageDirectory === settings.storageDirectory)
    )
      return;
    setSettings(next);
    setPendingSettings(next);
  }
  async function reload() {
    const s = await call<Store>("get_store");
    setStore(s);
    return s;
  }
  useEffect(() => {
    localStorage.setItem("hx-theme", darkMode ? "dark" : "light");
  }, [darkMode]);
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
          setSettingsReady(true);
        }
      })
      .catch((e) => {
        if (!disposed) notify(message(e), true);
      });
    void call<MemoryInfo>("system_memory")
      .then((info) => {
        if (!disposed) {
          setMemoryInfo(info);
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
      })
      .finally(() => {
        if (!disposed) setRestoringSession(false);
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);
  useEffect(() => {
    if (!desktop || !pendingSettings) return;
    let cancelled = false;
    const timeout = window.setTimeout(() => {
      void call("save_settings", { settings: pendingSettings }).catch(async (e) => {
        if (cancelled) return;
        notify(message(e), true);
        try {
          const saved = await call<Store>("get_store");
          if (!cancelled) setSettings(saved.settings);
        } catch (reloadError) {
          if (!cancelled) notify(message(reloadError), true);
        }
      });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [pendingSettings]);
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
    dismissToast();
    setProgress({
      phase: "prepare",
      message: "Préparation du pack…",
      current: 0,
      total: 0,
    });
    try {
      const instance = await call<Instance>("install_modpack", {
        path: pack.archivePath,
      });
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
  async function chooseStorageDirectory() {
    if (!desktop) {
      notify(
        "La sélection du dossier est disponible dans l’application Tauri.",
        true,
      );
      return;
    }
    try {
      const path = await open({ directory: true, multiple: false });
      if (typeof path === "string") {
        updateSettings({ ...settings, storageDirectory: path });
      }
    } catch (e) {
      notify(message(e), true);
    }
  }
  async function deleteInstance(instance: Instance) {
    if (
      !window.confirm(
        `Supprimer l’instance « ${instance.name} » et tous ses fichiers ?`,
      )
    ) {
      return;
    }
    setWorking(true);
    try {
      await call("delete_instance", { id: instance.id });
      if (selected === instance.id) setSelected("");
      await reload();
      notify(`${instance.name} a été supprimée.`);
    } catch (e) {
      notify(message(e), true);
    } finally {
      setWorking(false);
    }
  }
  async function chooseInstanceIcon(instance: Instance) {
    if (!desktop) {
      notify(
        "La personnalisation des icônes est disponible dans l’application Tauri.",
        true,
      );
      return;
    }
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Icône PNG", extensions: ["png"] }],
      });
      if (typeof path !== "string") return;
      await call("set_instance_icon", { id: instance.id, path });
      await reload();
      notify(`Icône de ${instance.name} mise à jour.`);
    } catch (e) {
      notify(message(e), true);
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
    <div
      className={`app-shell${darkMode ? " dark" : ""}`}
      onContextMenu={(event) => event.preventDefault()}
    >
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
          {navigationItems.map((n) => (
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
        <div className="toolbar-actions">
          <button
            className="account"
            disabled={busy || loginBusy || restoringSession}
            onClick={() => (profile ? setPage("settings") : void startLogin())}
          >
            {profile ? (
              <AccountAvatar key={profile.name} name={profile.name} />
            ) : (
              <UserRound size={16} />
            )}
            {restoringSession
              ? "Chargement du compte…"
              : (profile?.name ?? "Connexion Microsoft")}
            {profile && <span className="online-dot" />}
          </button>
          <button
            className="toolbar-action settings-button"
            aria-label="Ouvrir les réglages"
            title="Réglages"
            onClick={() => setPage("settings")}
          >
            <Settings2 size={16} />
          </button>
        </div>
      </div>
      {toast && (
        <div
          className={`notice toast ${toast.error ? "error" : ""}`}
          role={toast.error ? "alert" : "status"}
          aria-live="polite"
        >
          <span>{toast.text}</span>
          <button aria-label="Fermer la notification" onClick={dismissToast}>
            <X size={15} />
          </button>
        </div>
      )}
      <main
        key={page}
        className={animated ? "tab-slide" : undefined}
        data-direction={direction}
      >
        {page === "library" && (
          <section className="home">
            <div className="home-copy">
              <div className="launch-panel">
                <div className="instance-picker">
                  {chosen ? (
                    <div className="instance-picker-icon" aria-hidden="true">
                      <InstanceIcon instance={chosen} />
                    </div>
                  ) : (
                    <Box size={21} />
                  )}
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
                  disabled={busy || restoringSession}
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
            <Player
              profile={profile}
              onEditSkin={() =>
                notify("L’édition du skin sera bientôt disponible.")
              }
            />
          </section>
        )}
        {page === "packs" && (
          <section className="simple-page">
            <div className="page-heading page-heading-row">
              <h1>Instances.</h1>
              <div className="page-heading-actions">
                <button
                  className="button secondary header-icon-button"
                  disabled={working || busy}
                  aria-label="Créer une instance"
                  title="Créer une instance"
                  onClick={() => void createModal()}
                >
                  <Plus size={17} />
                </button>
                <button
                  className="button primary header-icon-button"
                  disabled={working || busy}
                  aria-label="Importer un modpack"
                  title="Importer un modpack"
                  onClick={() => void inspect()}
                >
                  {working ? (
                    <LoaderCircle className="spin" size={16} />
                  ) : (
                    <FileArchive size={16} />
                  )}
                </button>
              </div>
            </div>
            <div className="page-content">
              {!pack && !modpacks.length && (
                <div className="glass empty-state">
                  <FileArchive size={34} strokeWidth={1.2} />
                  <h2>Aucune instance installée</h2>
                  <p>
                    Importe un fichier ZIP CurseForge pour retrouver tes
                    modpacks ici.
                  </p>
                </div>
              )}
              {modpacks.map((instance) => (
                <div className="glass modpack-item" key={instance.id}>
                  <button
                    className={instance.iconPath ? "" : `instance-icon`}
                    type="button"
                    aria-label={`Modifier l’icône de ${instance.name}`}
                    title="Modifier l’icône"
                    disabled={working || busy}
                    onClick={() => void chooseInstanceIcon(instance)}
                  >
                    <InstanceIcon instance={instance} />
                  </button>
                  <div className="instance-info">
                    <h2>{instance.name}</h2>
                    <p>
                      Minecraft {instance.version} · {instance.loader} ·{" "}
                      {instance.modCount} mods
                    </p>
                  </div>
                  <div className="modpack-actions">
                    <button
                      className="button secondary play-instance-button"
                      disabled={working || busy}
                      aria-label={`Jouer à ${instance.name}`}
                      title="Jouer"
                      onClick={() => {
                        setSelected(instance.id);
                        setPage("library");
                      }}
                    >
                      <Play size={15} fill="currentColor" />
                    </button>
                    <button
                      className="icon-button danger-button"
                      disabled={working || busy}
                      aria-label={`Supprimer ${instance.name}`}
                      title="Supprimer l’instance"
                      onClick={() => void deleteInstance(instance)}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </div>
              ))}
              {pack && (
                <div className="glass pack-result">
                  <h2>{pack.name}</h2>
                  <p>
                    {pack.minecraft} · {pack.loader} · {pack.files.length} mods
                    · {pack.overrideCount} configurations
                  </p>
                  <button
                    className="button primary"
                    disabled={busy || working}
                    onClick={() => void installPack()}
                  >
                    {installing ? (
                      <LoaderCircle className="spin" size={17} />
                    ) : (
                      <ArrowDownToLine size={17} />
                    )}
                    {installing ? "Installation…" : "Installer"}
                  </button>
                  {installing && progress && (
                    <div className="inline-progress" role="status">
                      <span>{progress.message}</span>
                      <progress
                        max={progress.total || undefined}
                        value={progress.total ? progress.current : undefined}
                      />
                    </div>
                  )}
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
                <div className="account-actions">
                  <button
                    className="button secondary account-action"
                    disabled={busy || loginBusy || restoringSession}
                    onClick={() => void startLogin()}
                  >
                    <MicrosoftLogo />
                    {profile ? "Changer de compte" : "Se connecter"}
                  </button>
                  {profile && (
                    <button
                      className="subtle-link logout-button"
                      disabled={busy || restoringSession}
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
              </div>
              <div className="settings-grid">
                <label className="field">
                  Dossier de stockage
                  <div className="input-button">
                    <input
                      value={
                        settings.storageDirectory ||
                        settings.instancesDirectory ||
                        directory
                      }
                      readOnly
                      aria-describedby="instances-directory-help"
                    />
                    <button
                      type="button"
                      disabled={
                        busy || working || restoringSession || !settingsReady
                      }
                      onClick={() => void chooseStorageDirectory()}
                    >
                      Choisir
                    </button>
                  </div>
                  <small id="instances-directory-help">
                    {settings.instancesDirectory && !settings.storageDirectory
                      ? "Ancien dossier des instances. Choisis un dossier pour regrouper les instances et le runtime."
                      : "Contient instances/ (mondes et modpacks) et runtime/ (Java, assets et bibliothèques)."}
                  </small>
                </label>
                <label className="field">
                  Mémoire <strong>{settings.memoryMb / 1024} Go</strong>
                  <input
                    type="range"
                    min={1024}
                    max={Math.min(memoryInfo?.totalMb ?? 16384, 32768)}
                    step={512}
                    value={settings.memoryMb}
                    disabled={busy || restoringSession || !settingsReady}
                    onChange={(e) =>
                      updateSettings({
                        ...settings,
                        memoryMb: Number(e.target.value),
                      })
                    }
                  />
                  <small>
                    {memoryInfo
                      ? `${memoryInfo.totalMb / 1024} Go de mémoire totale détectée.`
                      : "Détection de la mémoire totale…"}
                  </small>
                </label>
              </div>
              <div className="theme-setting">
                <div>
                  <strong>Apparence</strong>
                  <span>{darkMode ? "Mode sombre" : "Mode clair"}</span>
                </div>
                <button
                  className="theme-toggle"
                  aria-label={
                    darkMode
                      ? "Activer le mode clair"
                      : "Activer le mode sombre"
                  }
                  title={darkMode ? "Mode clair" : "Mode sombre"}
                  onClick={() => setDarkMode((value) => !value)}
                >
                  {darkMode ? <Sun size={16} /> : <Moon size={16} />}
                </button>
              </div>
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
