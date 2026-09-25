import { useEffect, useState } from "react";
import type { Notice } from "../types";

const TOAST_DURATION = 4500;

export function useToast() {
  const [toast, setToast] = useState<Notice | null>(null);

  useEffect(() => {
    if (!toast) return;

    const timeout = window.setTimeout(() => setToast(null), TOAST_DURATION);
    return () => window.clearTimeout(timeout);
  }, [toast]);

  function showToast(text: string, error = false) {
    setToast({ text, error });
  }

  return {
    toast,
    showToast,
    dismissToast: () => setToast(null),
  };
}
