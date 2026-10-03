import { useEffect, useRef, useState } from "react";
import type { SkinViewer } from "skinview3d";
import { loadImage, type SkinModel } from "./texture";

type SkinPreviewProps = {
  texture: HTMLCanvasElement;
  revision: number;
  model: SkinModel;
  cape: string | null;
  outerLayer: boolean;
};

export default function SkinPreview({
  texture,
  revision,
  model,
  cape,
  outerLayer,
}: SkinPreviewProps) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [viewer, setViewer] = useState<SkinViewer | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let disposed = false;
    let created: SkinViewer | null = null;
    let observer: ResizeObserver | undefined;
    const pause = () => {
      if (created) created.renderPaused = document.hidden;
    };
    void import("skinview3d")
      .then(({ SkinViewer }) => {
        if (disposed || !canvas.current || !host.current) return;
        created = new SkinViewer({
          canvas: canvas.current,
          width: host.current.clientWidth,
          height: host.current.clientHeight,
          pixelRatio: Math.min(window.devicePixelRatio, 2),
        });
        created.fov = 40;
        created.zoom = 0.85;
        created.camera.position.set(22, 12, 60);
        created.controls.enablePan = false;
        created.controls.enableZoom = true;
        created.controls.minDistance = 20;
        created.controls.maxDistance = 90;
        observer = new ResizeObserver(() => {
          if (host.current)
            created?.setSize(
              host.current.clientWidth,
              host.current.clientHeight,
            );
        });
        observer.observe(host.current);
        document.addEventListener("visibilitychange", pause);
        setViewer(created);
      })
      .catch(() => {
        if (!disposed) setFailed(true);
      });
    return () => {
      disposed = true;
      observer?.disconnect();
      document.removeEventListener("visibilitychange", pause);
      created?.dispose();
    };
  }, []);

  useEffect(() => {
    if (!viewer) return;
    viewer.loadSkin(texture, { model });
    viewer.playerObject.skin.setOuterLayerVisible(outerLayer);
  }, [viewer, texture, revision, model, outerLayer]);

  useEffect(() => {
    if (!viewer) return;
    if (!cape) {
      viewer.resetCape();
      return;
    }
    // Load the image ourselves so a slower, older cape never wins the race.
    let cancelled = false;
    void loadImage(cape)
      .then((image) => {
        if (!cancelled) viewer.loadCape(image, { backEquipment: "cape" });
      })
      .catch(() => {
        if (!cancelled) viewer.resetCape();
      });
    return () => {
      cancelled = true;
    };
  }, [viewer, cape]);

  return (
    <div className="skin-preview" ref={host}>
      <canvas
        ref={canvas}
        role="img"
        aria-label="Aperçu 3D du skin en cours d’édition"
      />
      {failed && (
        <span className="player-caption">L’aperçu 3D nécessite WebGL.</span>
      )}
    </div>
  );
}
