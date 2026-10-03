import { useEffect, useRef } from "react";
import { loadImage } from "./texture";

type Area = [x: number, y: number, w: number, h: number];

// Head front plus hat, and the front of a cape, in 64-wide texture units.
export const HEAD_FRONT: Area[] = [
  [8, 8, 8, 8],
  [40, 8, 8, 8],
];
export const CAPE_FRONT: Area[] = [[1, 1, 10, 16]];

type TexturePatchProps = {
  source: string | null;
  areas: Area[];
  scale: number;
  className?: string;
};

/** Crisp pixel-art crop of a texture, scaled for HD textures as well. */
export default function TexturePatch({
  source,
  areas,
  scale,
  className,
}: TexturePatchProps) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [, , w, h] = areas[0];

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    const ctx = target.getContext("2d")!;
    ctx.clearRect(0, 0, target.width, target.height);
    if (!source) return;
    let cancelled = false;
    void loadImage(source)
      .then((image) => {
        if (cancelled) return;
        const unit = image.width / 64;
        ctx.imageSmoothingEnabled = false;
        for (const [x, y, aw, ah] of areas)
          ctx.drawImage(
            image,
            x * unit,
            y * unit,
            aw * unit,
            ah * unit,
            0,
            0,
            target.width,
            target.height,
          );
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [source, areas]);

  return (
    <canvas
      ref={canvas}
      className={className}
      width={w * scale}
      height={h * scale}
      aria-hidden="true"
    />
  );
}
