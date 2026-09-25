import { invoke, isTauri } from "@tauri-apps/api/core";

export const desktop = isTauri();

export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop) {
    throw new Error(
      "Ouvre l’application Tauri pour utiliser cette fonction : npm run tauri dev.",
    );
  }

  return invoke<T>(command, args);
}

export function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
