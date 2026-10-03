import { inferModelType, loadSkinToCanvas } from "skinview-utils";

export type SkinModel = "default" | "slim";
export type Rgba = [number, number, number, number];
export type Face = {
  part: string;
  side: string;
  overlay: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
};

export const TEXTURE_SIZE = 64;

// [label, u, v, width, height, depth, overlay] in the 1.8+ skin layout.
type Box = [string, number, number, number, number, number, boolean];
function boxes(model: SkinModel): Box[] {
  const arm = model === "slim" ? 3 : 4;
  return [
    ["Tête", 0, 0, 8, 8, 8, false],
    ["Tête", 32, 0, 8, 8, 8, true],
    ["Corps", 16, 16, 8, 12, 4, false],
    ["Corps", 16, 32, 8, 12, 4, true],
    ["Bras droit", 40, 16, arm, 12, 4, false],
    ["Bras droit", 40, 32, arm, 12, 4, true],
    ["Bras gauche", 32, 48, arm, 12, 4, false],
    ["Bras gauche", 48, 48, arm, 12, 4, true],
    ["Jambe droite", 0, 16, 4, 12, 4, false],
    ["Jambe droite", 0, 32, 4, 12, 4, true],
    ["Jambe gauche", 16, 48, 4, 12, 4, false],
    ["Jambe gauche", 0, 48, 4, 12, 4, true],
  ];
}

const faceCache = new Map<SkinModel, Face[]>();
export function skinFaces(model: SkinModel): Face[] {
  const cached = faceCache.get(model);
  if (cached) return cached;
  const faces = boxes(model).flatMap(([part, u, v, w, h, d, overlay]) =>
    (
      [
        ["dessus", u + d, v, w, d],
        ["dessous", u + d + w, v, w, d],
        ["droite", u, v + d, d, h],
        ["avant", u + d, v + d, w, h],
        ["gauche", u + d + w, v + d, d, h],
        ["arrière", u + 2 * d + w, v + d, w, h],
      ] as const
    ).map(([side, x, y, fw, fh]) => ({
      part,
      side,
      overlay,
      x,
      y,
      w: fw,
      h: fh,
    })),
  );
  faceCache.set(model, faces);
  return faces;
}

export function faceAt(x: number, y: number, model: SkinModel) {
  return skinFaces(model).find(
    (f) => x >= f.x && x < f.x + f.w && y >= f.y && y < f.y + f.h,
  );
}

export function createTexture() {
  const canvas = document.createElement("canvas");
  canvas.width = TEXTURE_SIZE;
  canvas.height = TEXTURE_SIZE;
  return canvas;
}

export function textureContext(canvas: HTMLCanvasElement) {
  return canvas.getContext("2d", { willReadFrequently: true })!;
}

export function loadImage(source: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Image illisible."));
    image.src = source;
  });
}

/** Normalizes legacy 64 × 32 skins to the editable 64 × 64 layout. */
export async function readSkin(source: string) {
  const image = await loadImage(source);
  if (
    image.width !== TEXTURE_SIZE ||
    (image.height !== TEXTURE_SIZE && image.height !== TEXTURE_SIZE / 2)
  ) {
    throw new Error("Le skin doit mesurer 64 × 64 ou 64 × 32 pixels.");
  }
  const canvas = createTexture();
  loadSkinToCanvas(canvas, image);
  return { canvas, model: inferModelType(canvas) as SkinModel };
}

export function hexToRgba(hex: string): Rgba {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255, 255];
}

export function rgbaToHex(pixel: ArrayLike<number>) {
  return `#${[pixel[0], pixel[1], pixel[2]].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

/** Contiguous fill bounded by the cube face, so colors never bleed across parts. */
export function floodFill(
  image: ImageData,
  startX: number,
  startY: number,
  color: Rgba,
  face: Face,
) {
  const { data, width } = image;
  const offset = (x: number, y: number) => (y * width + x) * 4;
  const start = offset(startX, startY);
  const target = [
    data[start],
    data[start + 1],
    data[start + 2],
    data[start + 3],
  ];
  if (target.every((value, i) => value === color[i])) return false;
  const matches = (o: number) =>
    data[o] === target[0] &&
    data[o + 1] === target[1] &&
    data[o + 2] === target[2] &&
    data[o + 3] === target[3];
  const stack = [[startX, startY]];
  while (stack.length) {
    const [x, y] = stack.pop()!;
    if (
      x < face.x ||
      y < face.y ||
      x >= face.x + face.w ||
      y >= face.y + face.h
    )
      continue;
    const o = offset(x, y);
    if (!matches(o)) continue;
    data.set(color, o);
    stack.push([x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]);
  }
  return true;
}

/** Bresenham line so fast strokes do not leave gaps between texels. */
export function texelLine(x0: number, y0: number, x1: number, y1: number) {
  const points: [number, number][] = [];
  const dx = Math.abs(x1 - x0);
  const dy = -Math.abs(y1 - y0);
  const sx = x0 < x1 ? 1 : -1;
  const sy = y0 < y1 ? 1 : -1;
  let error = dx + dy;
  for (;;) {
    points.push([x0, y0]);
    if (x0 === x1 && y0 === y1) return points;
    const e2 = 2 * error;
    if (e2 >= dy) {
      error += dy;
      x0 += sx;
    }
    if (e2 <= dx) {
      error += dx;
      y0 += sy;
    }
  }
}
