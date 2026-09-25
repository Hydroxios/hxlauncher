import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Edit3, LoaderCircle } from "lucide-react";
import type { SkinViewer } from "skinview3d";
export type PlayerProfile = {
  id: string;
  name: string;
  skins?: { url: string; variant: string; state: string }[];
  capes?: { url: string; state: string }[];
};

function accountSkin(profile: PlayerProfile | null) {
  const skins = profile?.skins ?? [];
  // Mojang currently returns ACTIVE/SLIM in uppercase, but keeping this
  // tolerant makes restored profiles work across API versions as well.
  return (
    skins.find((skin) => skin.state?.toUpperCase() === "ACTIVE") ?? skins[0]
  );
}

type PlayerProps = {
  profile: PlayerProfile | null;
  onEditSkin?: () => void;
};

export default function Player({ profile, onEditSkin }: PlayerProps) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewer = useRef<SkinViewer | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [fallback, setFallback] = useState(false);
  const [capeFailed, setCapeFailed] = useState(false);
  useEffect(() => {
    let disposed = false;
    let observer: ResizeObserver | undefined;
    let cleanup = () => {};
    setLoading(true);
    setFailed(false);
    setFallback(false);
    setCapeFailed(false);
    void (async () => {
      try {
        const { SkinViewer, IdleAnimation } = await import("skinview3d");
        if (disposed || !canvas.current || !host.current) return;
        const v = new SkinViewer({
          canvas: canvas.current,
          width: host.current.clientWidth,
          height: host.current.clientHeight,
          pixelRatio: Math.min(window.devicePixelRatio, 2),
        });
        viewer.current = v;
        v.fov = 38;
        v.zoom = 0.87;
        v.camera.position.set(24, 10, 64);
        v.controls.enableZoom = false;
        v.controls.enablePan = false;
        v.animation = new IdleAnimation();
        v.animation.speed = 0.7;
        v.autoRotate = false;
        const media = window.matchMedia("(prefers-reduced-motion: reduce)");
        const motion = () => {
          v.renderPaused = document.hidden;
          if (v.animation)
            v.animation.paused = media.matches || document.hidden;
        };
        motion();
        document.addEventListener("visibilitychange", motion);
        media.addEventListener("change", motion);
        cleanup = () => {
          document.removeEventListener("visibilitychange", motion);
          media.removeEventListener("change", motion);
        };
        observer = new ResizeObserver(() => {
          if (host.current)
            v.setSize(host.current.clientWidth, host.current.clientHeight);
        });
        observer.observe(host.current);
        const skin = accountSkin(profile);
        let source = "/skins/steve.png";
        let model: "default" | "slim" = "default";
        if (skin) {
          try {
            source =
              !isTauri() && skin.url.startsWith("/skins/")
                ? skin.url
                : await invoke<string>("skin_texture", {
                    url: skin.url.trim(),
                  });
            model = skin.variant?.toUpperCase() === "SLIM" ? "slim" : "default";
          } catch {
            if (!disposed) setFallback(true);
          }
        }
        if (disposed) return;
        try {
          await v.loadSkin(source, { model });
        } catch {
          if (!disposed) {
            setFallback(true);
            await v.loadSkin("/skins/steve.png", { model: "default" });
          }
        }
        if (disposed) return;
        const cape = profile?.capes?.find(
          (cape) => cape.state?.toUpperCase() === "ACTIVE",
        );
        if (cape) {
          try {
            const texture = await invoke<string>("skin_texture", {
              url: cape.url.trim(),
            });
            if (disposed) return;
            await v.loadCape(texture, { backEquipment: "cape" });
          } catch {
            if (!disposed) setCapeFailed(true);
          }
        }
        if (!disposed) v.render();
      } catch {
        if (!disposed) setFailed(true);
      } finally {
        if (!disposed) setLoading(false);
      }
    })();
    return () => {
      disposed = true;
      observer?.disconnect();
      cleanup();
      viewer.current?.dispose();
      viewer.current = null;
    };
  }, [profile]);
  return (
    <div
      className={`player-stage${loading ? " is-loading" : ""}`}
      aria-busy={loading}
    >
      {loading && (
        <div
          className="player-loader"
          role="status"
          aria-label="Chargement du personnage"
        >
          <LoaderCircle className="spin" size={28} />
        </div>
      )}
      <div className="player-orbit" />
      <div className="player-shadow" />
      <button
        className="player-edit-button"
        type="button"
        aria-label="Modifier le skin"
        title="Modifier le skin"
        onClick={onEditSkin}
      >
        <Edit3 size={16} />
      </button>
      <div className="player-canvas" ref={host}>
        <canvas
          ref={canvas}
          aria-label={`Personnage Minecraft animé : ${profile?.name ?? "Steve"}`}
          role="img"
        />
      </div>
      {failed ? (
        <span className="player-caption">L’aperçu 3D nécessite WebGL.</span>
      ) : fallback ? (
        <span className="player-caption">Skin indisponible.</span>
      ) : capeFailed ? (
        <span className="player-caption">Cape indisponible.</span>
      ) : null}
    </div>
  );
}
