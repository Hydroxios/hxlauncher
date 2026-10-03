import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  Brush,
  Check,
  Download,
  Eraser,
  Grid3x3,
  Layers2,
  LoaderCircle,
  PaintBucket,
  Pipette,
  Redo2,
  Save,
  Frame,
  Trash2,
  Undo2,
  Upload,
} from "lucide-react";
import { accountSkin, type PlayerProfile } from "../Player";
import { call, desktop, getErrorMessage } from "../lib/tauri";
import SkinCanvas, { type Tool } from "./SkinCanvas";
import SkinPreview from "./SkinPreview";
import TexturePatch, { CAPE_FRONT, HEAD_FRONT } from "./TexturePatch";
import {
  TEXTURE_SIZE,
  createTexture,
  readSkin,
  skinFaces,
  textureContext,
  type SkinModel,
} from "./texture";

type Tab = "draw" | "library" | "cape";
type SavedSkin = { id: string; name: string; variant: string; data: string };
type Snapshot = { image: ImageData; model: SkinModel };
type Start = { source: string; model: SkinModel };

const STEVE = "/skins/steve.png";
const DRAFT_KEY = "hx-skin-draft";
const HISTORY_LIMIT = 100;
const PALETTE = [
  "#f6d2b4",
  "#e0a878",
  "#b5774c",
  "#6b4227",
  "#2b1d14",
  "#000000",
  "#3d3a44",
  "#8a8494",
  "#ffffff",
  "#c8323c",
  "#e8902b",
  "#f1d84b",
  "#4caf50",
  "#2f7fd0",
  "#6d3fb5",
  "#e46aa8",
];
const tools: { id: Tool; label: string; key: string; icon: typeof Brush }[] = [
  { id: "pencil", label: "Crayon", key: "B", icon: Brush },
  { id: "eraser", label: "Gomme", key: "E", icon: Eraser },
  { id: "fill", label: "Remplissage", key: "G", icon: PaintBucket },
  { id: "picker", label: "Pipette", key: "I", icon: Pipette },
];

// Mojang texture URLs are content-addressed, so a session-wide cache is safe.
const textures = new Map<string, Promise<string>>();
function mojangTexture(url: string) {
  const key = url.trim();
  if (!desktop && key.startsWith("/skins/")) return Promise.resolve(key);
  let texture = textures.get(key);
  if (!texture) {
    texture = call<string>("skin_texture", { url: key });
    texture.catch(() => textures.delete(key));
    textures.set(key, texture);
  }
  return texture;
}

async function accountStart(
  profile: PlayerProfile | null,
): Promise<Start | null> {
  const skin = accountSkin(profile);
  if (!skin) return null;
  return {
    source: await mojangTexture(skin.url),
    model: skin.variant?.toUpperCase() === "SLIM" ? "slim" : "default",
  };
}

function readDraft(): Start | null {
  try {
    const draft = JSON.parse(localStorage.getItem(DRAFT_KEY) ?? "null");
    return typeof draft?.source === "string" &&
      (draft.model === "default" || draft.model === "slim")
      ? draft
      : null;
  } catch {
    return null;
  }
}

function fileName(name: string) {
  return `${name.replace(/[\\/:*?"<>|]/g, "").trim() || "skin"}.png`;
}

const message = getErrorMessage;

type SkinEditorProps = {
  profile: PlayerProfile | null;
  onProfileChange: (profile: PlayerProfile) => void;
  onRequireLogin: () => void;
  notify: (text: string, error?: boolean) => void;
  log: (text: string) => void;
};

export default function SkinEditor({
  profile,
  onProfileChange,
  onRequireLogin,
  notify,
  log,
}: SkinEditorProps) {
  const [texture] = useState(createTexture);
  const [revision, setRevision] = useState(0);
  const [ready, setReady] = useState(false);
  const [model, setModel] = useState<SkinModel>("default");
  const [tab, setTab] = useState<Tab>("draw");
  const [tool, setTool] = useState<Tool>("pencil");
  const [color, setColor] = useState("#6b4227");
  const [recent, setRecent] = useState<string[]>([]);
  const [grid, setGrid] = useState(true);
  const [guide, setGuide] = useState(true);
  const [outerLayer, setOuterLayer] = useState(true);
  const [history, setHistory] = useState({ undo: 0, redo: 0 });
  const [applying, setApplying] = useState(false);
  const [account, setAccount] = useState<Start | null>(null);
  const [saved, setSaved] = useState<SavedSkin[]>([]);
  const [libraryId, setLibraryId] = useState<string | null>(null);
  const [skinName, setSkinName] = useState("");
  const [saving, setSaving] = useState(false);
  const [capeChoice, setCapeChoice] = useState<string | null>(null);
  const [capeTextures, setCapeTextures] = useState<Record<string, string>>({});
  const [equipping, setEquipping] = useState(false);
  const undoStack = useRef<Snapshot[]>([]);
  const redoStack = useRef<Snapshot[]>([]);
  const frame = useRef(0);
  const fileInput = useRef<HTMLInputElement>(null);
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});

  const capes = profile?.capes ?? [];
  const activeCape =
    capes.find((cape) => cape.state?.toUpperCase() === "ACTIVE")?.id ?? "";
  const selectedCape = capeChoice ?? activeCape;
  const previewCape = selectedCape
    ? (capeTextures[selectedCape] ?? null)
    : null;

  function changed() {
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      setRevision((value) => value + 1);
    });
  }
  function syncHistory() {
    setHistory({
      undo: undoStack.current.length,
      redo: redoStack.current.length,
    });
  }
  function snapshot(): Snapshot {
    return {
      image: textureContext(texture).getImageData(
        0,
        0,
        TEXTURE_SIZE,
        TEXTURE_SIZE,
      ),
      model,
    };
  }
  function commit(image: ImageData) {
    undoStack.current.push({ image, model });
    if (undoStack.current.length > HISTORY_LIMIT) undoStack.current.shift();
    redoStack.current = [];
    syncHistory();
  }
  function restore(from: Snapshot[], to: Snapshot[]) {
    const previous = from.pop();
    if (!previous) return;
    to.push(snapshot());
    textureContext(texture).putImageData(previous.image, 0, 0);
    setModel(previous.model);
    syncHistory();
    changed();
  }
  const undo = () => restore(undoStack.current, redoStack.current);
  const redo = () => restore(redoStack.current, undoStack.current);

  /** Replaces the whole texture as one undoable step. */
  function replaceTexture(source: HTMLCanvasElement | null, next: SkinModel) {
    const ctx = textureContext(texture);
    commit(ctx.getImageData(0, 0, TEXTURE_SIZE, TEXTURE_SIZE));
    ctx.clearRect(0, 0, TEXTURE_SIZE, TEXTURE_SIZE);
    if (source) ctx.drawImage(source, 0, 0);
    setModel(next);
    changed();
  }
  async function loadStart(start: Start, label: string) {
    try {
      const { canvas } = await readSkin(start.source);
      replaceTexture(canvas, start.model);
      setTab("draw");
      notify(`${label} chargé. ⌘Z / Ctrl+Z pour annuler.`);
    } catch (e) {
      notify(message(e), true);
    }
  }
  function startBlank() {
    const blank = createTexture();
    const ctx = textureContext(blank);
    ctx.fillStyle = "#c9c3d1";
    for (const face of skinFaces(model))
      if (!face.overlay) ctx.fillRect(face.x, face.y, face.w, face.h);
    replaceTexture(blank, model);
    setLibraryId(null);
    setSkinName("");
    setTab("draw");
  }
  function chooseColor(next: string) {
    setColor(next);
    if (tool === "eraser" || tool === "picker") setTool("pencil");
  }
  function paintCommitted(before: ImageData) {
    commit(before);
    if (tool !== "pencil" && tool !== "fill") return;
    setRecent((colors) =>
      [color, ...colors.filter((value) => value !== color)].slice(0, 8),
    );
  }

  // Initial texture: the unsaved draft first, then the account skin, then Steve.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let start = readDraft();
      if (!start) start = await accountStart(profile).catch(() => null);
      let loaded: Awaited<ReturnType<typeof readSkin>>;
      try {
        loaded = await readSkin(start?.source ?? STEVE);
      } catch {
        start = null;
        loaded = await readSkin(STEVE);
      }
      if (cancelled) return;
      textureContext(texture).drawImage(loaded.canvas, 0, 0);
      setModel(start?.model ?? "default");
      setReady(true);
      changed();
    })().catch(() => {
      if (!cancelled) setReady(true);
    });
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame.current);
      frame.current = 0;
    };
    // The starting texture is chosen once; later profile changes must not
    // overwrite work in progress.
  }, []);

  useEffect(() => {
    if (!ready) return;
    const timeout = window.setTimeout(() => {
      try {
        localStorage.setItem(
          DRAFT_KEY,
          JSON.stringify({ source: texture.toDataURL("image/png"), model }),
        );
      } catch {
        // The draft is a convenience; editing keeps working without it.
      }
    }, 400);
    return () => window.clearTimeout(timeout);
  }, [ready, revision, model, texture]);

  useEffect(() => {
    let cancelled = false;
    setAccount(null);
    void accountStart(profile)
      .then((start) => {
        if (!cancelled) setAccount(start);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [profile]);

  useEffect(() => {
    let cancelled = false;
    for (const cape of profile?.capes ?? []) {
      if (!cape.id) continue;
      void mojangTexture(cape.url)
        .then((source) => {
          if (!cancelled)
            setCapeTextures((current) => ({ ...current, [cape.id!]: source }));
        })
        .catch(() => {});
    }
    return () => {
      cancelled = true;
    };
  }, [profile]);

  // Cached sessions may predate cape ids; fetch the live profile once.
  useEffect(() => {
    if (!desktop || !profile) return;
    let cancelled = false;
    void call<PlayerProfile>("reload_profile")
      .then((fresh) => {
        if (!cancelled) onProfileChange(fresh);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [profile?.id]);

  useEffect(() => {
    if (!desktop) return;
    let cancelled = false;
    void call<SavedSkin[]>("list_skins")
      .then((skins) => {
        if (!cancelled) setSaved(skins);
      })
      .catch((e) => {
        if (!cancelled) notify(message(e), true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  keyHandler.current = (event: KeyboardEvent) => {
    const target = event.target;
    if (
      target instanceof Element &&
      target.closest("input, select, textarea, [role='dialog']")
    )
      return;
    const key = event.key.toLowerCase();
    if (event.metaKey || event.ctrlKey) {
      if (key === "z") {
        event.preventDefault();
        if (event.shiftKey) redo();
        else undo();
      } else if (key === "y") {
        event.preventDefault();
        redo();
      }
      return;
    }
    if (tab !== "draw" || event.altKey) return;
    const shortcut = tools.find((item) => item.key.toLowerCase() === key);
    if (shortcut) setTool(shortcut.id);
    else if (key === "x") setGrid((value) => !value);
  };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyHandler.current(event);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  async function uploadSkin(data: string, variant: string) {
    if (!profile) {
      onRequireLogin();
      return;
    }
    setApplying(true);
    try {
      const updated = await call<PlayerProfile>("upload_skin", {
        data,
        variant,
      });
      onProfileChange(updated);
      notify("Skin appliqué sur ton compte Minecraft.");
      log("Skin mis à jour sur le compte Minecraft.");
    } catch (e) {
      notify(message(e), true);
    } finally {
      setApplying(false);
    }
  }

  async function importSkin() {
    if (!desktop) {
      fileInput.current?.click();
      return;
    }
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Skin PNG", extensions: ["png"] }],
      });
      if (typeof path !== "string") return;
      const source = await call<string>("read_skin_file", { path });
      const { canvas, model: inferred } = await readSkin(source);
      replaceTexture(canvas, inferred);
      setLibraryId(null);
      setTab("draw");
      notify("Skin importé.");
    } catch (e) {
      notify(message(e), true);
    }
  }

  function importBrowserFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      void readSkin(String(reader.result))
        .then(({ canvas, model: inferred }) => {
          replaceTexture(canvas, inferred);
          setLibraryId(null);
          setTab("draw");
          notify("Skin importé.");
        })
        .catch((e) => notify(message(e), true));
    };
    reader.readAsDataURL(file);
  }

  async function exportSkin() {
    const data = texture.toDataURL("image/png");
    const name = fileName(skinName || profile?.name || "skin");
    if (!desktop) {
      const link = document.createElement("a");
      link.href = data;
      link.download = name;
      link.click();
      return;
    }
    try {
      const path = await save({
        defaultPath: name,
        filters: [{ name: "Skin PNG", extensions: ["png"] }],
      });
      if (!path) return;
      await call("export_skin", { path, data });
      notify("Skin exporté.");
    } catch (e) {
      notify(message(e), true);
    }
  }

  async function saveToLibrary(copy = false) {
    const name = skinName.trim();
    if (!name) {
      notify("Donne un nom à ton skin pour l’enregistrer.", true);
      return;
    }
    setSaving(true);
    try {
      const entry = await call<SavedSkin>("save_skin", {
        id: copy ? null : libraryId,
        name,
        variant: model,
        data: texture.toDataURL("image/png"),
      });
      setSaved((skins) => [
        entry,
        ...skins.filter((skin) => skin.id !== entry.id),
      ]);
      setLibraryId(entry.id);
      notify(`« ${entry.name} » enregistré dans ta bibliothèque.`);
    } catch (e) {
      notify(message(e), true);
    } finally {
      setSaving(false);
    }
  }

  async function openSaved(skin: SavedSkin) {
    try {
      const { canvas } = await readSkin(skin.data);
      replaceTexture(canvas, skin.variant === "slim" ? "slim" : "default");
      setLibraryId(skin.id);
      setSkinName(skin.name);
      setTab("draw");
    } catch (e) {
      notify(message(e), true);
    }
  }

  async function deleteSaved(skin: SavedSkin) {
    if (!window.confirm(`Supprimer « ${skin.name} » de ta bibliothèque ?`))
      return;
    try {
      await call("delete_skin", { id: skin.id });
      setSaved((skins) => skins.filter((item) => item.id !== skin.id));
      if (libraryId === skin.id) setLibraryId(null);
    } catch (e) {
      notify(message(e), true);
    }
  }

  async function equipCape() {
    setEquipping(true);
    try {
      const updated = await call<PlayerProfile>("set_cape", {
        id: selectedCape || null,
      });
      onProfileChange(updated);
      setCapeChoice(null);
      notify(selectedCape ? "Cape équipée." : "Cape retirée.");
    } catch (e) {
      notify(message(e), true);
    } finally {
      setEquipping(false);
    }
  }

  const palette = [
    ...recent,
    ...PALETTE.filter((value) => !recent.includes(value)),
  ];

  return (
    <section className="simple-page skin-page">
      <div className="page-heading page-heading-row">
        <h1>Skin.</h1>
        <div className="page-heading-actions">
          <button
            className="button secondary header-icon-button"
            aria-label="Importer un skin PNG"
            title="Importer un skin PNG"
            disabled={!ready}
            onClick={() => void importSkin()}
          >
            <Upload size={16} />
          </button>
          <button
            className="button secondary header-icon-button"
            aria-label="Exporter le skin en PNG"
            title="Exporter en PNG"
            disabled={!ready}
            onClick={() => void exportSkin()}
          >
            <Download size={16} />
          </button>
          <button
            className="button primary skin-apply"
            disabled={!ready || applying}
            onClick={() =>
              void uploadSkin(
                texture.toDataURL("image/png"),
                model === "slim" ? "slim" : "classic",
              )
            }
          >
            {applying ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Check size={16} />
            )}
            {profile ? "Appliquer au compte" : "Se connecter pour appliquer"}
          </button>
          <input
            ref={fileInput}
            type="file"
            accept="image/png"
            hidden
            onChange={importBrowserFile}
          />
        </div>
      </div>
      <div className="skin-layout">
        <div className="glass skin-preview-card">
          <SkinPreview
            texture={texture}
            revision={revision}
            model={model}
            cape={previewCape}
            outerLayer={outerLayer}
          />
          {!ready && (
            <div
              className="player-loader"
              role="status"
              aria-label="Chargement du skin"
            >
              <LoaderCircle className="spin" size={26} />
            </div>
          )}
          <div className="skin-preview-controls">
            <div
              className="skin-segmented"
              role="group"
              aria-label="Modèle des bras"
            >
              {(
                [
                  ["default", "Classique"],
                  ["slim", "Fin"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  aria-pressed={model === value}
                  className={model === value ? "active" : ""}
                  onClick={() => setModel(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            <button
              className={`skin-chip${outerLayer ? " active" : ""}`}
              aria-pressed={outerLayer}
              title="Afficher le 2e calque dans l’aperçu"
              onClick={() => setOuterLayer((value) => !value)}
            >
              <Layers2 size={14} />
              2e calque
            </button>
          </div>
        </div>
        <div className="glass skin-workspace">
          <div
            className="skin-tabs"
            role="tablist"
            aria-label="Éditeur de skin"
          >
            {(
              [
                ["draw", "Dessiner"],
                ["library", "Bibliothèque"],
                ["cape", "Cape"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                className={tab === id ? "active" : ""}
                onClick={() => setTab(id)}
              >
                {label}
              </button>
            ))}
          </div>
          {tab === "draw" && (
            <div className="skin-draw" role="tabpanel">
              <div className="skin-tools" role="toolbar" aria-label="Outils">
                {tools.map((item) => (
                  <button
                    key={item.id}
                    className={tool === item.id ? "active" : ""}
                    aria-pressed={tool === item.id}
                    aria-label={`${item.label} (${item.key})`}
                    title={`${item.label} (${item.key})`}
                    onClick={() => setTool(item.id)}
                  >
                    <item.icon size={16} />
                  </button>
                ))}
                <hr />
                <button
                  aria-label="Annuler"
                  title="Annuler (⌘Z)"
                  disabled={!history.undo}
                  onClick={undo}
                >
                  <Undo2 size={16} />
                </button>
                <button
                  aria-label="Rétablir"
                  title="Rétablir (⇧⌘Z)"
                  disabled={!history.redo}
                  onClick={redo}
                >
                  <Redo2 size={16} />
                </button>
                <hr />
                <button
                  className={grid ? "active" : ""}
                  aria-pressed={grid}
                  aria-label="Grille (X)"
                  title="Grille (X)"
                  onClick={() => setGrid((value) => !value)}
                >
                  <Grid3x3 size={16} />
                </button>
                <button
                  className={guide ? "active" : ""}
                  aria-pressed={guide}
                  aria-label="Contours des faces"
                  title="Contours des faces"
                  onClick={() => setGuide((value) => !value)}
                >
                  <Frame size={16} />
                </button>
              </div>
              <SkinCanvas
                texture={texture}
                revision={revision}
                model={model}
                tool={tool}
                color={color}
                grid={grid}
                guide={guide}
                onChange={changed}
                onCommit={paintCommitted}
                onPick={(picked) => {
                  setColor(picked);
                  if (tool === "picker") setTool("pencil");
                }}
              />
              <div className="skin-colors">
                <label
                  className="skin-color-current"
                  title="Choisir une couleur"
                >
                  <input
                    type="color"
                    value={color}
                    onChange={(event) => chooseColor(event.target.value)}
                    aria-label="Couleur du crayon"
                  />
                  <span style={{ background: color }} aria-hidden="true" />
                  <code>{color.toUpperCase()}</code>
                </label>
                <div className="skin-palette" role="group" aria-label="Palette">
                  {palette.map((value) => (
                    <button
                      key={value}
                      className={value === color ? "active" : ""}
                      style={{ background: value }}
                      aria-label={`Couleur ${value}`}
                      title={value.toUpperCase()}
                      onClick={() => chooseColor(value)}
                    />
                  ))}
                </div>
              </div>
            </div>
          )}
          {tab === "library" && (
            <div className="skin-library" role="tabpanel">
              <h3>Point de départ</h3>
              <div className="skin-starters">
                {account && (
                  <button
                    onClick={() => void loadStart(account, "Ton skin actuel")}
                  >
                    <TexturePatch
                      source={account.source}
                      areas={HEAD_FRONT}
                      scale={4}
                    />
                    Mon skin actuel
                  </button>
                )}
                <button
                  onClick={() =>
                    void loadStart({ source: STEVE, model: "default" }, "Steve")
                  }
                >
                  <TexturePatch source={STEVE} areas={HEAD_FRONT} scale={4} />
                  Steve
                </button>
                <button onClick={startBlank}>
                  <span className="skin-blank" aria-hidden="true" />
                  Vierge
                </button>
              </div>
              <h3>Mes skins</h3>
              <div className="skin-save-row">
                <input
                  value={skinName}
                  maxLength={40}
                  placeholder="Nom du skin"
                  aria-label="Nom du skin"
                  onChange={(event) => setSkinName(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") void saveToLibrary();
                  }}
                />
                <button
                  className="button primary"
                  disabled={saving || !ready || !skinName.trim()}
                  onClick={() => void saveToLibrary()}
                >
                  {saving ? (
                    <LoaderCircle className="spin" size={15} />
                  ) : (
                    <Save size={15} />
                  )}
                  {libraryId ? "Mettre à jour" : "Enregistrer"}
                </button>
                {libraryId && (
                  <button
                    className="button secondary"
                    disabled={saving || !skinName.trim()}
                    onClick={() => void saveToLibrary(true)}
                  >
                    Copie
                  </button>
                )}
              </div>
              {saved.length ? (
                <div className="skin-saved">
                  {saved.map((skin) => (
                    <div
                      className={`skin-saved-item${skin.id === libraryId ? " current" : ""}`}
                      key={skin.id}
                    >
                      <button
                        className="skin-saved-open"
                        title="Ouvrir dans l’éditeur"
                        onClick={() => void openSaved(skin)}
                      >
                        <TexturePatch
                          source={skin.data}
                          areas={HEAD_FRONT}
                          scale={4}
                        />
                        <span>
                          <strong>{skin.name}</strong>
                          <small>
                            {skin.variant === "slim" ? "Fin" : "Classique"}
                          </small>
                        </span>
                      </button>
                      <button
                        className="icon-button"
                        aria-label={`Appliquer ${skin.name} au compte`}
                        title="Appliquer au compte"
                        disabled={applying}
                        onClick={() => void uploadSkin(skin.data, skin.variant)}
                      >
                        <Check size={15} />
                      </button>
                      <button
                        className="icon-button danger-button"
                        aria-label={`Supprimer ${skin.name}`}
                        title="Supprimer"
                        onClick={() => void deleteSaved(skin)}
                      >
                        <Trash2 size={15} />
                      </button>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="skin-empty">
                  {desktop
                    ? "Enregistre tes créations pour les retrouver ici et les appliquer en un clic."
                    : "La bibliothèque est disponible dans l’application Tauri."}
                </p>
              )}
            </div>
          )}
          {tab === "cape" && (
            <div className="skin-capes" role="tabpanel">
              {!profile ? (
                <div className="skin-empty">
                  <p>
                    Connecte-toi pour choisir parmi les capes de ton compte.
                  </p>
                  <button className="button secondary" onClick={onRequireLogin}>
                    Se connecter
                  </button>
                </div>
              ) : !capes.length ? (
                <p className="skin-empty">
                  Aucune cape sur ce compte. Les capes obtenues lors
                  d’événements Minecraft apparaîtront ici.
                </p>
              ) : (
                <>
                  <div
                    className="skin-cape-grid"
                    role="radiogroup"
                    aria-label="Capes"
                  >
                    <button
                      role="radio"
                      aria-checked={!selectedCape}
                      className={!selectedCape ? "active" : ""}
                      onClick={() => setCapeChoice("")}
                    >
                      <span className="skin-cape-none" aria-hidden="true" />
                      Aucune
                    </button>
                    {capes.map((cape) => (
                      <button
                        key={cape.id || cape.url}
                        role="radio"
                        aria-checked={selectedCape === cape.id}
                        className={selectedCape === cape.id ? "active" : ""}
                        disabled={!cape.id}
                        onClick={() => setCapeChoice(cape.id ?? "")}
                      >
                        <TexturePatch
                          source={
                            cape.id ? (capeTextures[cape.id] ?? null) : null
                          }
                          areas={CAPE_FRONT}
                          scale={4}
                          className="skin-cape-thumb"
                        />
                        {cape.alias || "Cape"}
                        {cape.id === activeCape && <small>Équipée</small>}
                      </button>
                    ))}
                  </div>
                  <button
                    className="button primary skin-cape-apply"
                    disabled={equipping || selectedCape === activeCape}
                    onClick={() => void equipCape()}
                  >
                    {equipping ? (
                      <LoaderCircle className="spin" size={15} />
                    ) : (
                      <Check size={15} />
                    )}
                    {selectedCape ? "Équiper cette cape" : "Retirer la cape"}
                  </button>
                </>
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
