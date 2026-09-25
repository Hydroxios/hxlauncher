import { useEffect, useRef, useState } from "react";
import { Check, Copy, LoaderCircle, RefreshCw } from "lucide-react";

export default function CopyCodeButton({ code }: { code: string }) {
  const [status, setStatus] = useState<"idle" | "copying" | "copied" | "error">(
    "idle",
  );
  const mounted = useRef(true);
  const reset = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      clearTimeout(reset.current);
    };
  }, []);

  async function copy() {
    clearTimeout(reset.current);
    setStatus("copying");
    try {
      await navigator.clipboard.writeText(code);
      if (!mounted.current) return;
      setStatus("copied");
      reset.current = setTimeout(() => setStatus("idle"), 2000);
    } catch {
      if (mounted.current) setStatus("error");
    }
  }

  const label =
    status === "copied"
      ? "Copié !"
      : status === "error"
        ? "Réessayer"
        : status === "copying"
          ? "Copie…"
          : "Copier";
  return (
    <button
      className={`copy-code ${status}`}
      type="button"
      disabled={status === "copying"}
      aria-label={
        status === "error"
          ? "Copie impossible. Réessayer de copier le code"
          : status === "copied"
            ? "Code copié"
            : "Copier le code"
      }
      title={
        status === "error"
          ? "Copie impossible. Tu peux aussi sélectionner le code manuellement."
          : undefined
      }
      onClick={() => void copy()}
    >
      {status === "copied" ? (
        <Check size={16} />
      ) : status === "copying" ? (
        <LoaderCircle className="spin" size={16} />
      ) : status === "error" ? (
        <RefreshCw size={16} />
      ) : (
        <Copy size={16} />
      )}
      <span role="status" aria-live="polite">
        {label}
      </span>
    </button>
  );
}
