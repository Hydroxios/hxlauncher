import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LoaderCircle } from "lucide-react";
import type { SkinViewer } from "skinview3d";
export type PlayerProfile = {
  id: string;
  name: string;
  skins?: { url: string; variant: string; state: string }[];
};
export default function Player({ profile }: { profile: PlayerProfile | null }) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewer = useRef<SkinViewer | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [fallback, setFallback] = useState(false);
  useEffect(() => {
    let disposed = false;
    let observer: ResizeObserver | undefined;
    let cleanup = () => {};
    setLoading(true);
    setFailed(false);
    setFallback(false);
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
        const skin =
          profile?.skins?.find((s) => s.state === "ACTIVE") ??
          profile?.skins?.[0];
        let source = "/skins/steve.png";
        let model: "default" | "slim" = "default";
        if (skin) {
          try {
            source = await invoke<string>("skin_texture", { url: skin.url });
            model = skin.variant === "SLIM" ? "slim" : "default";
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
    <div className={`player-stage${loading ? " is-loading" : ""}`} aria-busy={loading}>
      {loading && <div className="player-loader" role="status" aria-label="Chargement du personnage"><LoaderCircle className="spin" size={28} /></div>}
      <div className="player-orbit" />
      <div className="player-shadow" />
      <div className="player-canvas" ref={host}>
        <canvas
          ref={canvas}
          title="Glisser pour tourner"
          aria-label={`Personnage Minecraft animé : ${profile?.name ?? "Steve"}`}
          role="img"
        />
      </div>
      {failed ? (
        <span className="player-caption">L’aperçu 3D nécessite WebGL.</span>
      ) : fallback ? (
        <span className="player-caption">Skin indisponible.</span>
      ) : null}
    </div>
  );
}
