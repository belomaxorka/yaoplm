import type { ArtCacheInfo } from "@/api/generated/ArtCacheInfo";
import type { Attribution } from "@/api/generated/Attribution";
import type { CfgView } from "@/api/generated/CfgView";
import type { DeviceInfo } from "@/api/generated/DeviceInfo";
import type { DirListing } from "@/api/generated/DirListing";
import type { GameRef } from "@/api/generated/GameRef";
import type { JobId } from "@/api/generated/JobId";
import type { JobInfo } from "@/api/generated/JobInfo";
import type { JobResult } from "@/api/generated/JobResult";
import type { Library } from "@/api/generated/Library";
import type { LoaderBootHint } from "@/api/generated/LoaderBootHint";
import type { LoaderCatalog } from "@/api/generated/LoaderCatalog";
import type { LoaderFile } from "@/api/generated/LoaderFile";
import type { LoaderOffer } from "@/api/generated/LoaderOffer";
import type { LoaderProfile } from "@/api/generated/LoaderProfile";
import type { PlanView } from "@/api/generated/PlanView";
import type { ProfileOverride } from "@/api/generated/ProfileOverride";
import type { RedumpInfo } from "@/api/generated/RedumpInfo";
import type { Request } from "@/api/generated/Request";
import type { VmcCard } from "@/api/generated/VmcCard";
import { call, RpcError, type Transport } from "@/api/transport";

/** Typed wrappers over `call`; every screen goes through these. */

/** The profiles chosen per device; every other device is read (LIB-1, ADR-0040). */
export interface ProfileChoice {
  overrides: ProfileOverride[];
}

export function listDevices(transport: Transport, choice: ProfileChoice): Promise<DeviceInfo[]> {
  return call(transport, { type: "ListDevices", ...choice }, "Devices").then((r) => r.devices);
}

export async function inspectDevice(transport: Transport, root: string, choice: ProfileChoice): Promise<DeviceInfo> {
  const { type: _type, ...device } = await call(transport, { type: "InspectDevice", root, ...choice }, "Device");
  return device;
}

export async function listDir(transport: Transport, path: string | null): Promise<DirListing> {
  const { type: _type, ...listing } = await call(
    transport,
    path === null ? { type: "ListDir" } : { type: "ListDir", path },
    "Dir",
  );
  return listing;
}

export async function requestPlan(transport: Transport, request: Request): Promise<PlanView> {
  const { type: _type, ...plan } = await call(transport, request, "Plan");
  return plan;
}

export function executePlan(transport: Transport, plan: string, confirmDelete: boolean): Promise<JobId> {
  return call(transport, { type: "ExecutePlan", plan, confirmDelete }, "Job").then((r) => r.job);
}

export async function discardPlan(transport: Transport, plan: string): Promise<void> {
  await call(transport, { type: "DiscardPlan", plan }, "Ack");
}

export async function getJob(transport: Transport, job: JobId): Promise<JobInfo> {
  const { type: _type, ...info } = await call(transport, { type: "GetJob", job }, "JobState");
  return info;
}

export function listJobs(transport: Transport): Promise<JobInfo[]> {
  return call(transport, { type: "ListJobs" }, "Jobs").then((r) => r.jobs);
}

export async function cancelJob(transport: Transport, job: JobId): Promise<void> {
  await call(transport, { type: "CancelJob", job }, "Ack");
}

/** Starts a cover search (ART-1, ART-4); the job ends with a plan to confirm. */
export function fetchArt(transport: Transport, request: Extract<Request, { type: "FetchArt" }>): Promise<JobId> {
  return call(transport, request, "Job").then((r) => r.job);
}

/** Starts looking for every picture of one type for one game, to pick from (ART-2). */
export function findArt(transport: Transport, request: Extract<Request, { type: "FindArt" }>): Promise<JobId> {
  return call(transport, request, "Job").then((r) => r.job);
}

/** Starts reading picture sets (ART-5); the job ends with a plan to confirm. */
export function importArt(transport: Transport, request: Extract<Request, { type: "ImportArt" }>): Promise<JobId> {
  return call(transport, request, "Job").then((r) => r.job);
}

/** Starts hashing games and comparing them with the Redump DAT (GAME-5). */
export function verifyGames(transport: Transport, request: Extract<Request, { type: "VerifyGames" }>): Promise<JobId> {
  return call(transport, request, "Job").then((r) => r.job);
}

/** The stored Redump DAT, or null before the first download or import (META-2). */
export function redumpStatus(transport: Transport): Promise<RedumpInfo | null> {
  return call(transport, { type: "RedumpStatus" }, "Redump").then((r) => r.dat);
}

/** Downloads the DAT (at most weekly) or imports `file` instead. */
export function updateRedump(transport: Transport, file?: string): Promise<JobId> {
  return call(transport, file ? { type: "UpdateRedump", file } : { type: "UpdateRedump" }, "Job").then((r) => r.job);
}

/** A game's CFG: its settings and every key (CFG-2). */
export async function readCfg(transport: Transport, root: string, game: GameRef): Promise<CfgView> {
  const { type: _type, ...view } = await call(transport, { type: "ReadCfg", root, game }, "Cfg");
  return view;
}

/** The memory cards in VMC/, with OPL's verdict and the games using each (VMC-1). */
export function listVmc(transport: Transport, root: string): Promise<VmcCard[]> {
  return call(transport, { type: "ListVmc", root }, "Vmcs").then((r) => r.cards);
}

export async function attribution(transport: Transport): Promise<Attribution> {
  const { type: _type, ...about } = await call(transport, { type: "Attribution" }, "Attribution");
  return about;
}

export async function artCache(transport: Transport): Promise<ArtCacheInfo> {
  const { type: _type, ...info } = await call(transport, { type: "ArtCache" }, "ArtCache");
  return info;
}

export async function clearArtCache(transport: Transport): Promise<void> {
  await call(transport, { type: "ClearArtCache" }, "Ack");
}

/** The loader catalogue in force (LDR-1). */
export async function loaderCatalog(transport: Transport): Promise<LoaderCatalog> {
  const { type: _type, ...catalog } = await call(transport, { type: "LoaderCatalog" }, "LoaderCatalog");
  return catalog;
}

/** Runs a loader job to its end and returns what it ended with. */
async function loaderJob(transport: Transport, request: Request, signal?: AbortSignal): Promise<JobResult> {
  const { job } = await call(transport, request, "Job");
  const info = await waitForJob(transport, job, signal);
  if (info.status === "finished" && info.result) return info.result;
  throw new RpcError(info.error ?? { code: "Internal", message: "the job ended without a result" });
}

/** What a download of `build` would be (LDR-2). */
export async function loaderRelease(
  transport: Transport,
  build: string,
  mirror: string,
  signal?: AbortSignal,
): Promise<LoaderOffer> {
  const result = await loaderJob(transport, { type: "LoaderRelease", build, ...(mirror ? { mirror } : {}) }, signal);
  if (result.type !== "LoaderOffer") throw new RpcError({ code: "Internal", message: `unexpected ${result.type}` });
  const { type: _type, ...offer } = result;
  return offer;
}

/** Downloads the build the offer named, held to the hash it showed (LDR-2). */
export async function downloadLoader(
  transport: Transport,
  offer: LoaderOffer,
  languages: boolean,
  mirror: string,
  signal?: AbortSignal,
): Promise<LoaderFile> {
  const result = await loaderJob(
    transport,
    {
      type: "DownloadLoader",
      build: offer.build,
      tag: offer.tag,
      ...(offer.file.sha256 ? { sha256: offer.file.sha256 } : {}),
      ...(languages ? { languages } : {}),
      ...(mirror ? { mirror } : {}),
    },
    signal,
  );
  if (result.type !== "LoaderFile") throw new RpcError({ code: "Internal", message: `unexpected ${result.type}` });
  const { type: _type, ...file } = result;
  return file;
}

/** How the console starts an installed loader (BOOT-1). */
export function loaderBootHints(
  transport: Transport,
  request: Omit<Extract<Request, { type: "LoaderBootHints" }>, "type">,
): Promise<LoaderBootHint[]> {
  return call(transport, { type: "LoaderBootHints", ...request }, "LoaderBootHints").then((r) => r.hints);
}

const FINAL = new Set(["finished", "failed", "cancelled"]);

/** Polls `GetJob` until the job is over. Progress itself arrives as events. */
export async function waitForJob(transport: Transport, job: JobId, signal?: AbortSignal): Promise<JobInfo> {
  let delay = 100;
  for (;;) {
    const info = await getJob(transport, job);
    if (FINAL.has(info.status)) return info;
    if (signal?.aborted) throw new RpcError({ code: "Cancelled", message: "cancelled" });
    await new Promise((resolve) => setTimeout(resolve, delay));
    delay = Math.min(delay * 2, 1000);
  }
}

/**
 * Starts a scan and waits for the library it produces. A scan nobody waits
 * for any more (`signal` aborted: replaced by a newer one) is cancelled on
 * the core too, so a device is never read twice at once.
 */
export async function scanLibrary(
  transport: Transport,
  root: string,
  profile: LoaderProfile,
  verify: boolean,
  signal?: AbortSignal,
): Promise<Library> {
  const { job } = await call(transport, { type: "ScanLibrary", root, profile, verify }, "Job");
  const stop = () => void cancelJob(transport, job).catch(() => undefined);
  if (signal?.aborted) stop();
  signal?.addEventListener("abort", stop, { once: true });
  try {
    const info = await waitForJob(transport, job, signal);
    if (info.status === "finished" && info.result?.type === "Library") {
      const { type: _type, ...library } = info.result;
      return library;
    }
    throw new RpcError(info.error ?? { code: "Internal", message: "the scan produced no library" });
  } finally {
    signal?.removeEventListener("abort", stop);
  }
}
