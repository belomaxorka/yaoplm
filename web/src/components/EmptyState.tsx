import type { LucideIcon } from "lucide-react";

export function EmptyState({
  icon: Icon,
  title,
  description,
}: {
  icon: LucideIcon;
  title: string;
  description: string;
}) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center gap-3 py-20 text-center">
      <div className="flex size-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <Icon className="size-6" aria-hidden />
      </div>
      <h2 className="text-base font-medium">{title}</h2>
      <p className="text-sm text-balance text-muted-foreground">{description}</p>
    </div>
  );
}
