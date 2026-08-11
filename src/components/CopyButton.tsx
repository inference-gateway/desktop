import { useEffect, useState } from "react";
import { Check, Copy, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export function CopyButton({ text }: { text: string }) {
  const [status, setStatus] = useState<"idle" | "copied" | "error">("idle");

  const copy = () => {
    if (status === "copied") return;
    navigator.clipboard.writeText(text).then(
      () => setStatus("copied"),
      () => setStatus("error"),
    );
  };

  useEffect(() => {
    if (status === "idle") return;
    const t = setTimeout(() => setStatus("idle"), 5000);
    return () => clearTimeout(t);
  }, [status]);

  const Icon = status === "copied" ? Check : status === "error" ? X : Copy;

  return (
    <Button
      type="button"
      size="icon-sm"
      variant="secondary"
      onClick={copy}
      disabled={status === "copied"}
      aria-label="Copy message text"
      className={cn(
        "self-end transition-opacity",
        status === "copied" && "text-green-600 dark:text-green-500",
        status === "error" && "text-destructive",
      )}
    >
      <Icon />
    </Button>
  );
}
