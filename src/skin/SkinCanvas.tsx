import { useEffect, useRef, useState, type PointerEvent } from "react";
import {
  TEXTURE_SIZE,
  faceAt,
  floodFill,
  hexToRgba,
  rgbaToHex,
  skinFaces,
  texelLine,
  textureContext,
  type SkinModel,
} from "./texture";

export type Tool = "pencil" | "eraser" | "fill" | "picker";

type Stroke = { x: number; y: number; before: ImageData; changed: boolean };

type SkinCanvasProps = {
  texture: HTMLCanvasElement;
  revision: number;
  model: SkinModel;
  tool: Tool;
  color: string;
  grid: boolean;
  guide: boolean;
  onChange: () => void;
  onCommit: (before: ImageData) => void;
  onPick: (color: string) => void;
};

function themeColor(element: Element, name: string) {
  return (
    getComputedStyle(element).getPropertyValue(name).trim() || "transparent"
  );
}

export default function SkinCanvas({
  texture,
  revision,
  model,
  tool,
  color,
  grid,
  guide,
  onChange,
  onCommit,
  onPick,
}: SkinCanvasProps) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<HTMLCanvasElement>(null);
  const stroke = useRef<Stroke | null>(null);
  const [size, setSize] = useState(0);
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    // Whole CSS pixels per texel keep every texel the same size on screen.
    const observer = new ResizeObserver(() => {
      const { width, height } = element.getBoundingClientRect();
      const side = Math.min(width, height);
      setSize(Math.max(1, Math.floor(side / TEXTURE_SIZE)) * TEXTURE_SIZE);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const canvas = view.current;
    if (!canvas || !size) return;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const pixels = Math.round(size * ratio);
    if (canvas.width !== pixels) {
      canvas.width = pixels;
      canvas.height = pixels;
    }
    const ctx = canvas.getContext("2d")!;
    const texel = pixels / TEXTURE_SIZE;
    const faces = skinFaces(model);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, pixels, pixels);

    // Each texel shows a small checkerboard where it is transparent.
    const tile = document.createElement("canvas");
    tile.width = tile.height = Math.max(2, Math.round(texel));
    const tileCtx = tile.getContext("2d")!;
    const half = tile.width / 2;
    tileCtx.fillStyle = themeColor(canvas, "--checker-a");
    tileCtx.fillRect(0, 0, tile.width, tile.width);
    tileCtx.fillStyle = themeColor(canvas, "--checker-b");
    tileCtx.fillRect(0, 0, half, half);
    tileCtx.fillRect(half, half, half, half);
    ctx.fillStyle = ctx.createPattern(tile, "repeat")!;
    for (const f of faces)
      ctx.fillRect(f.x * texel, f.y * texel, f.w * texel, f.h * texel);

    ctx.drawImage(texture, 0, 0, pixels, pixels);

    if (grid) {
      ctx.strokeStyle = themeColor(canvas, "--grid");
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let i = 1; i < TEXTURE_SIZE; i++) {
        const at = Math.round(i * texel) + 0.5;
        ctx.moveTo(at, 0);
        ctx.lineTo(at, pixels);
        ctx.moveTo(0, at);
        ctx.lineTo(pixels, at);
      }
      ctx.stroke();
    }
    if (guide) {
      ctx.lineWidth = Math.max(1, ratio);
      for (const f of faces) {
        ctx.strokeStyle = themeColor(
          canvas,
          f.overlay ? "--guide-overlay" : "--guide",
        );
        ctx.strokeRect(
          f.x * texel + ctx.lineWidth / 2,
          f.y * texel + ctx.lineWidth / 2,
          f.w * texel - ctx.lineWidth,
          f.h * texel - ctx.lineWidth,
        );
      }
    }
    if (hover && faceAt(hover.x, hover.y, model)) {
      ctx.lineWidth = Math.max(1, ratio);
      ctx.strokeStyle = themeColor(canvas, "--cursor");
      ctx.strokeRect(
        hover.x * texel + ctx.lineWidth / 2,
        hover.y * texel + ctx.lineWidth / 2,
        texel - ctx.lineWidth,
        texel - ctx.lineWidth,
      );
    }
  }, [texture, revision, size, model, grid, guide, hover]);

  function texelAt(event: PointerEvent<HTMLCanvasElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = Math.floor(
      ((event.clientX - rect.left) / rect.width) * TEXTURE_SIZE,
    );
    const y = Math.floor(
      ((event.clientY - rect.top) / rect.height) * TEXTURE_SIZE,
    );
    if (x < 0 || y < 0 || x >= TEXTURE_SIZE || y >= TEXTURE_SIZE) return null;
    return { x, y };
  }

  function paint(x: number, y: number, erase: boolean) {
    if (!faceAt(x, y, model)) return false;
    const ctx = textureContext(texture);
    const pixel = ctx.getImageData(x, y, 1, 1).data;
    if (erase) {
      if (pixel[3] === 0) return false;
      ctx.clearRect(x, y, 1, 1);
      return true;
    }
    const target = hexToRgba(color);
    if (target.every((value, i) => pixel[i] === value)) return false;
    ctx.putImageData(new ImageData(new Uint8ClampedArray(target), 1, 1), x, y);
    return true;
  }

  function pick(x: number, y: number) {
    const pixel = textureContext(texture).getImageData(x, y, 1, 1).data;
    if (pixel[3] > 0) onPick(rgbaToHex(pixel));
  }

  function pointerDown(event: PointerEvent<HTMLCanvasElement>) {
    const point = texelAt(event);
    if (!point || (event.button !== 0 && event.button !== 2)) return;
    if (event.button === 2 || event.altKey || tool === "picker") {
      pick(point.x, point.y);
      return;
    }
    const ctx = textureContext(texture);
    const before = ctx.getImageData(0, 0, TEXTURE_SIZE, TEXTURE_SIZE);
    if (tool === "fill") {
      const face = faceAt(point.x, point.y, model);
      if (!face) return;
      const image = ctx.getImageData(0, 0, TEXTURE_SIZE, TEXTURE_SIZE);
      if (floodFill(image, point.x, point.y, hexToRgba(color), face)) {
        ctx.putImageData(image, 0, 0);
        onCommit(before);
        onChange();
      }
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    const changed = paint(point.x, point.y, tool === "eraser");
    stroke.current = { ...point, before, changed };
    if (changed) onChange();
  }

  function pointerMove(event: PointerEvent<HTMLCanvasElement>) {
    const point = texelAt(event);
    setHover((current) =>
      current?.x === point?.x && current?.y === point?.y ? current : point,
    );
    const current = stroke.current;
    if (!current || !point) return;
    if (point.x === current.x && point.y === current.y) return;
    let changed = false;
    for (const [x, y] of texelLine(current.x, current.y, point.x, point.y))
      changed = paint(x, y, tool === "eraser") || changed;
    current.x = point.x;
    current.y = point.y;
    if (changed) {
      current.changed = true;
      onChange();
    }
  }

  function pointerUp() {
    const current = stroke.current;
    stroke.current = null;
    if (current?.changed) onCommit(current.before);
  }

  const face = hover ? faceAt(hover.x, hover.y, model) : undefined;
  return (
    <div className="skin-canvas-frame">
      <div className="skin-canvas-host" ref={host}>
        <canvas
          ref={view}
          className={`skin-canvas tool-${tool}`}
          style={{ width: size, height: size }}
          aria-label="Texture du skin, 64 × 64 pixels"
          onPointerDown={pointerDown}
          onPointerMove={pointerMove}
          onPointerUp={pointerUp}
          onPointerCancel={pointerUp}
          onPointerLeave={() => setHover(null)}
        />
      </div>
      <p className="skin-canvas-status" aria-live="polite">
        {face && hover ? (
          <>
            <strong>{face.part}</strong> · {face.side}
            {face.overlay && <span className="skin-layer-tag">2e calque</span>}
            <span className="skin-coords">
              {hover.x}, {hover.y}
            </span>
          </>
        ) : (
          "Clic droit ou Alt : pipette · Glisse pour dessiner"
        )}
      </p>
    </div>
  );
}
