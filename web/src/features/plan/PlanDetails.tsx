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
  MemoryStick,
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
import { useSimpleMode } from "@/settings/context";

const stepIcons: Record<StepOp, LucideIcon> = {
  createDir: FolderPlus,
  removeDir: FolderMinus,
  writeFile: FileInput,
  writeZso: FileArchive,
  writeVmc: MemoryStick,
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
        {step.removal === "backup" && <span className="block text-xs text-muted-foreground">{t("plan.backup")}</span>}
      </span>
      {step.bytes !== null && step.bytes > 0 && (
        <span className="shrink-0 font-mono text-xs text-muted-foreground tabular-nums">
          {step.op === "writeZso" ? t("plan.atMost", { size: formatBytes(step.bytes) }) : formatBytes(step.bytes)}
        </span>
      )}
    </li>
  );
}

/**
 * One operation of a plan. Its steps (folders made, files renamed, written,
 * deleted) are a detail: folded away by default, and left out in simple
 * mode, where the summary, the notices and the confirmation are enough.
 */
export function TransactionCard({
  index,
  transaction,
  children,
}: {
  index: number;
  transaction: PlanTransaction;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
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
      {transaction.cfgChanges.length > 0 && (
        <div className="mt-2" data-testid="plan-cfg-changes">
          <p className="text-xs text-muted-foreground">{t("plan.cfgChanges")}</p>
          <ul className="mt-0.5 flex flex-col gap-0.5 font-mono text-xs">
            {transaction.cfgChanges.map((change) => (
              <li key={change.key} className="wrap-anywhere">
                {change.key}: {change.before ?? t("plan.cfgAbsent")} → {change.after ?? t("plan.cfgRemoved")}
              </li>
            ))}
          </ul>
        </div>
      )}
      {!simple && transaction.steps.length > 0 && (
        <details className="group mt-2" data-testid="plan-steps">
          <summary className="cursor-pointer text-xs text-muted-foreground select-none hover:text-foreground">
            {t("plan.stepsToggle", { count: transaction.steps.length })}
          </summary>
          <ul className="mt-1 flex flex-col gap-0.5">
            {transaction.steps.map((step, i) => (
              <StepLine key={i} step={step} />
            ))}
          </ul>
        </details>
      )}
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
  keyOf = (_transaction, index) => String(index),
}: {
  plan: PlanView;
  /** Extra controls inside a transaction card, e.g. import overrides. */
  renderTransaction?: (transaction: PlanTransaction, index: number) => ReactNode;
  /**
   * A key that stays with a transaction across re-plans, so controls in its
   * card keep their focus when transactions are inserted before it.
   */
  keyOf?: (transaction: PlanTransaction, index: number) => string;
}) {
  const { t } = useTranslation();
  const simple = useSimpleMode();
  const blockers = plan.notices.filter((n) => n.level === "error");
  // Simple mode lists the games an operation touches and the loader it puts
  // on the device, not the housekeeping around them (backup folders, ul.cfg,
  // the manifest): the summary counts it all. A plan of housekeeping alone
  // (creating OPL's folders) lists it: that is all it does.
  const all = plan.transactions.map((transaction, index) => ({ transaction, index }));
  const essential = all.filter(({ transaction }) => transaction.game !== null || transaction.loader === true);
  const shown = !simple || essential.length === 0 ? all : essential;
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
      {shown.length > 0 && (
        <section>
          <h3 className="mb-2 text-sm font-medium">{t("plan.transactions", { count: shown.length })}</h3>
          <ul className="flex flex-col gap-2">
            {shown.map(({ transaction, index }, position) => (
              <TransactionCard key={keyOf(transaction, index)} index={position} transaction={transaction}>
                {renderTransaction?.(transaction, index)}
              </TransactionCard>
            ))}
          </ul>
        </section>
      )}
      {plan.skipped.length > 0 &&
        // A bulk picture download skips far more than it writes: simple
        // mode keeps the list behind its count.
        (simple ? (
          <details data-testid="plan-skipped-details">
            <summary className="cursor-pointer text-sm font-medium">{t("plan.skipped", { count: plan.skipped.length })}</summary>
            <SkippedList skipped={plan.skipped} />
          </details>
        ) : (
          <section data-testid="plan-skipped">
            <h3 className="mb-2 text-sm font-medium">{t("plan.skipped", { count: plan.skipped.length })}</h3>
            <SkippedList skipped={plan.skipped} />
          </section>
        ))}
    </div>
  );
}

function SkippedList({ skipped }: { skipped: PlanView["skipped"] }) {
  return (
    <ul className="flex flex-col gap-1.5">
      {skipped.map((item) => (
        <li key={item.path} className="rounded-md border px-2 py-1.5 text-sm">
          <div className="truncate font-mono text-xs" title={item.path}>
            {item.path}
          </div>
          <NoticeList notices={[item.reason]} />
        </li>
      ))}
    </ul>
  );
}
