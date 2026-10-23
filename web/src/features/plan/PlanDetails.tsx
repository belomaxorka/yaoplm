import { cn } from "cn";
import {
  ArrowRightLeft,
  Copy,
  FileArchive,
  FileInput,
  FilePen,
  FolderMinus,
  FolderPlus,
  type LucideIcon,
  NotebookPen,
  Trash2,
} from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { PlanStep } from "@/api/generated/PlanStep";
import type { PlanTransaction } from "@/api/generated/PlanTransaction";
import type { PlanView } from "@/api/generated/PlanView";
import type { StepOp } from "@/api/generated/StepOp";
import { NoticeList } from "@/components/Notices";
import { Badge } from "@/components/ui/badge";
import { formatBytes } from "@/lib/format";
import { phraseText } from "@/lib/phrase";

const stepIcons: Record<StepOp, LucideIcon> = {
  createDir: FolderPlus,
  removeDir: FolderMinus,
  writeFile: FileInput,
  writeZso: FileArchive,
  putFile: NotebookPen,
  copyFile: Copy,
  rename: FilePen,
  remove: Trash2,
  removeSource: Trash2,
  dropJournal: NotebookPen,
};

function StepLine({ step }: { step: PlanStep }) {
  const { t } = useTranslation();
  const Icon = step.op === "rename" && step.to ? ArrowRightLeft : stepIcons[step.op];
  const destructive = step.removal === "userData";
  return (
    <li
      className={cn(
        "flex items-start gap-2 rounded-md px-2 py-1 text-sm",
        destructive && "bg-destructive/10 text-destructive",
      )}
      data-op={step.op}
      data-removal={step.removal ?? undefined}
    >
      <Icon className="mt-0.5 size-4 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1 wrap-anywhere">
        <span>{t(`plan.steps.${step.op}`, { path: step.path, to: step.to ?? "" })}</span>
        {(step.op === "writeFile" || step.op === "writeZso") && step.source && (
          <span className="block text-xs text-muted-foreground">{t("plan.from", { source: step.source })}</span>
        )}
        {step.removal === "cache" && <span className="block text-xs text-muted-foreground">{t("plan.cache")}</span>}
      </span>
      {step.bytes !== null && step.bytes > 0 && (
        <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
          {step.op === "writeZso" ? t("plan.atMost", { size: formatBytes(step.bytes) }) : formatBytes(step.bytes)}
        </span>
      )}
    </li>
  );
}

export function TransactionCard({
  index,
  transaction,
  children,
}: {
  index: number;
  transaction: PlanTransaction;
  children?: ReactNode;
}) {
  return (
    <li className="rounded-lg border bg-card p-3" data-testid="plan-transaction">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-mono text-xs text-muted-foreground">{index + 1}.</span>
        <span className="font-medium">{phraseText(transaction.titlePhrase, transaction.title)}</span>
        {transaction.game && (
          <span className="ml-auto flex items-center gap-1">
            <Badge variant="outline">{transaction.game.media}</Badge>
            <Badge variant="outline">{transaction.game.format}</Badge>
            <Badge variant="secondary" className="font-mono">
              {transaction.game.startup}
            </Badge>
          </span>
        )}
      </div>
      {children}
      <ul className="mt-2 flex flex-col gap-0.5">
        {transaction.steps.map((step, i) => (
          <StepLine key={i} step={step} />
        ))}
      </ul>
      <NoticeList notices={transaction.notices} className="mt-2" />
    </li>
  );
}

export function PlanSummary({ plan }: { plan: PlanView }) {
  const { t } = useTranslation();
  const after = plan.freeBytes - plan.writeBytes + plan.deleteBytes;
  // A ZSO's size is known only once it is written: the plan counts the worst case.
  const estimated = plan.transactions.some((tx) => tx.steps.some((step) => step.op === "writeZso"));
  const writes = formatBytes(plan.writeBytes);
  const free = formatBytes(Math.max(0, after));
  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-sm sm:grid-cols-4" data-testid="plan-summary">
      <div>
        <dt className="text-xs text-muted-foreground">{t("plan.writes")}</dt>
        <dd className="font-mono tabular-nums">{estimated ? t("plan.atMost", { size: writes }) : writes}</dd>
      </div>
      <div>
        <dt className="text-xs text-muted-foreground">{t("plan.deletes")}</dt>
        <dd className={cn("font-mono tabular-nums", plan.deleteBytes > 0 && "font-semibold text-destructive")}>
          {formatBytes(plan.deleteBytes)}
        </dd>
      </div>
      <div>
        <dt className="text-xs text-muted-foreground">{t("plan.freeNow")}</dt>
        <dd className="font-mono tabular-nums">{formatBytes(plan.freeBytes)}</dd>
      </div>
      <div>
        <dt className="text-xs text-muted-foreground">{t("plan.freeAfter")}</dt>
        <dd className={cn("font-mono tabular-nums", after < 0 && "font-semibold text-destructive")}>
          {estimated ? t("plan.atLeast", { size: free }) : free}
        </dd>
      </div>
    </dl>
  );
}

export function PlanDetails({
  plan,
  renderTransaction,
}: {
  plan: PlanView;
  /** Extra controls inside a transaction card, e.g. import overrides. */
  renderTransaction?: (transaction: PlanTransaction, index: number) => ReactNode;
}) {
  const { t } = useTranslation();
  const blockers = plan.notices.filter((n) => n.level === "error");
  return (
    <div className="flex flex-col gap-4">
      <PlanSummary plan={plan} />
      {blockers.length > 0 && (
        <div className="rounded-lg border border-destructive/40 bg-destructive/10 p-3" role="alert">
          <p className="mb-1 text-sm font-medium text-destructive">{t("plan.blocked")}</p>
          <NoticeList notices={blockers} />
        </div>
      )}
      <NoticeList notices={plan.notices.filter((n) => n.level !== "error")} />
      {plan.transactions.length > 0 && (
        <section>
          <h3 className="mb-2 text-sm font-medium">
            {t("plan.transactions", { count: plan.transactions.length })}
          </h3>
          <ul className="flex flex-col gap-2">
            {plan.transactions.map((transaction, index) => (
              <TransactionCard key={index} index={index} transaction={transaction}>
                {renderTransaction?.(transaction, index)}
              </TransactionCard>
            ))}
          </ul>
        </section>
      )}
      {plan.skipped.length > 0 && (
        <section data-testid="plan-skipped">
          <h3 className="mb-2 text-sm font-medium">{t("plan.skipped", { count: plan.skipped.length })}</h3>
          <ul className="flex flex-col gap-1.5">
            {plan.skipped.map((skipped) => (
              <li key={skipped.path} className="rounded-md border px-2 py-1.5 text-sm">
                <div className="truncate font-mono text-xs" title={skipped.path}>
                  {skipped.path}
                </div>
                <NoticeList notices={[skipped.reason]} compact />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
