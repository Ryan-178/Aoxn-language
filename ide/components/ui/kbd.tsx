import { cn } from "@/components/lib/utils";

function Kbd({ className, ...props }: React.ComponentProps<"kbd">) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        "pointer-events-none inline-flex h-4 min-w-4 shrink-0 select-none items-center justify-center gap-1 rounded-md bg-tooltip-tag px-1.5 font-mono text-ui-xs font-medium text-tooltip-tag-foreground",
        className,
      )}
      {...props}
    />
  );
}

/** 键帽组合容器：把一次组合的多个键横排（gap-1），如 ⌘ + K。 */
function KbdGroup({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="kbd-group"
      className={cn("inline-flex items-center gap-1", className)}
      {...props}
    />
  );
}

export { Kbd, KbdGroup };
