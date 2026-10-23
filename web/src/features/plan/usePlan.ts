import { useCallback, useEffect, useRef, useState } from "react";

import { useApi } from "@/api/context";
import type { JobId } from "@/api/generated/JobId";
import type { PlanView } from "@/api/generated/PlanView";
import type { Request } from "@/api/generated/Request";
import { discardPlan, executePlan, requestPlan } from "@/api/rpc";
import { RpcError } from "@/api/transport";
import { useJobs } from "@/jobs/context";

export interface PlanState {
  plan: PlanView | null;
  /** A plan request is in flight. */
  pending: boolean;
  /** `ExecutePlan` is in flight. */
  executing: boolean;
  error: string | null;
}

const idle: PlanState = { plan: null, pending: false, executing: false, error: null };

function message(error: unknown): string {
  if (error instanceof RpcError) return error.message;
  return String(error);
}

/**
 * Plan → confirm → execute (SPEC §6, ADR-0021). Every write in the UI goes
 * through this hook: the core builds the plan, the user sees it, and only
 * its id goes back. A plan the user walks away from is discarded.
 */
export function usePlan(options: { onExecuted?: (job: JobId, plan: PlanView) => void } = {}) {
  const { transport } = useApi();
  const { markExecuted } = useJobs();
  const [state, setState] = useState<PlanState>(idle);
  const onExecuted = useRef(options.onExecuted);
  onExecuted.current = options.onExecuted;
  const current = useRef<PlanView | null>(null);
  const generation = useRef(0);

  const forget = useCallback(() => {
    const plan = current.current;
    current.current = null;
    if (plan) void discardPlan(transport, plan.id).catch(() => undefined);
  }, [transport]);

  const request = useCallback(
    async (req: Request): Promise<PlanView | null> => {
      forget();
      const mine = ++generation.current;
      setState({ plan: null, pending: true, executing: false, error: null });
      try {
        const plan = await requestPlan(transport, req);
        if (mine !== generation.current) {
          void discardPlan(transport, plan.id).catch(() => undefined);
          return null;
        }
        current.current = plan;
        setState({ plan, pending: false, executing: false, error: null });
        return plan;
      } catch (error) {
        if (mine === generation.current) setState({ plan: null, pending: false, executing: false, error: message(error) });
        return null;
      }
    },
    [transport, forget],
  );

  const discard = useCallback(() => {
    generation.current += 1;
    forget();
    setState(idle);
  }, [forget]);

  /** Takes over a plan another request produced, e.g. the result of a FetchArt job. */
  const adopt = useCallback(
    (plan: PlanView) => {
      forget();
      generation.current += 1;
      current.current = plan;
      setState({ plan, pending: false, executing: false, error: null });
    },
    [forget],
  );

  const execute = useCallback(
    async (confirmDelete: boolean) => {
      const plan = current.current;
      if (!plan) return;
      setState((s) => ({ ...s, executing: true, error: null }));
      try {
        const job = await executePlan(transport, plan.id, confirmDelete);
        current.current = null;
        markExecuted(plan.id);
        setState(idle);
        onExecuted.current?.(job, plan);
      } catch (error) {
        setState((s) => ({ ...s, executing: false, error: message(error) }));
      }
    },
    [transport, markExecuted],
  );

  // Leaving the screen discards a plan that was never confirmed.
  useEffect(() => forget, [forget]);

  return { ...state, request, adopt, discard, execute };
}
