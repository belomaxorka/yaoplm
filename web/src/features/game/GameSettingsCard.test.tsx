import "@/i18n";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";

import { ApiProvider } from "@/api/ApiProvider";
import type { CfgView } from "@/api/generated/CfgView";
import type { Game } from "@/api/generated/Game";
import type { PlanView } from "@/api/generated/PlanView";
import type { Request } from "@/api/generated/Request";
import type { Response } from "@/api/generated/Response";
import type { VmcCard } from "@/api/generated/VmcCard";
import { MockTransport, RpcError } from "@/api/transport";
import { TooltipProvider } from "@/components/ui/tooltip";
import { GameSettingsCard } from "@/features/game/GameSettingsCard";
import i18n from "@/i18n";
import { JobsProvider } from "@/jobs/JobsProvider";
import { SettingsProvider } from "@/settings/SettingsProvider";

const ROOT = "E:\\";

const game: Game = {
  title: "Retail Game",
  startup: "SLUS_200.62",
  media: "CD",
  format: "ISO",
  path: "E:\\CD\\SLUS_200.62.Retail Game.iso",
  nameFormat: "old",
  parts: [],
  ulIndex: null,
  size: 700_000_000,
  imageStartup: null,
  layer1Start: null,
  cfg: { path: "E:\\CFG\\SLUS_200.62.cfg", oplSyntax: true, compatibility: 0x41, vmc: [null, null] },
  art: [],
  cheats: null,
  warnings: [],
  knownTitle: null,
  dump: null,
};

const cfg: CfgView = {
  startup: "SLUS_200.62",
  path: "E:\\CFG\\SLUS_200.62.cfg",
  exists: true,
  editable: true,
  // Mode 1, and 0.9.3's mode 7 that stock OPL does not know.
  settings: {
    compatibility: 0x41,
    dma: null,
    altStartup: "",
    vmc: ["", ""],
    gsm: { perGame: false, enabled: false, vmode: 0, xOffset: 0, yOffset: 0, fieldFix: false },
    cheats: { perGame: false, enabled: false, mode: 0 },
    padEmu: { perGame: false, enabled: false, settings: 0 },
  },
  entries: [{ key: "$Compatibility", value: "65" }],
  cardNames: ["SLUS_200.62_0", "SLUS_200.62_1"],
};

/** A core with the game's CFG and one card OPL refuses; it records WriteCfg. */
function core(cards: VmcCard[]) {
  const written: Extract<Request, { type: "WriteCfg" }>[] = [];
  const transport = new MockTransport((request: Request): Response => {
    switch (request.type) {
      case "ReadCfg":
        return { type: "Cfg", ...cfg };
      case "ListVmc":
        return { type: "Vmcs", cards };
      case "ListJobs":
        return { type: "Jobs", jobs: [] };
      case "WriteCfg": {
        written.push(request);
        const plan: PlanView = {
          id: "p1",
          kind: "settings",
          title: "Settings of Retail Game",
          root: ROOT,
          writeBytes: 64,
          deleteBytes: 0,
          freeBytes: 1 << 30,
          executable: true,
          deletesUserData: false,
          transactions: [
            {
              title: "Settings of Retail Game",
              game: { title: "Retail Game", startup: "SLUS_200.62", media: "CD", format: "ISO" },
              steps: [],
              commit: 0,
              notices: [],
              cfgChanges: [{ key: "$Compatibility", before: "65", after: "67" }],
            },
          ],
          skipped: [],
          notices: [],
        };
        return { type: "Plan", ...plan };
      }
      case "DiscardPlan":
        return { type: "Ack" };
      default:
        throw new RpcError({ code: "NotFound", message: `${request.type} is not faked` });
    }
  });
  return { transport, written };
}

function show(transport: MockTransport, simpleMode: boolean) {
  window.localStorage.setItem("yaoplm.settings", JSON.stringify({ root: ROOT, simpleMode }));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider transport={transport}>
        <SettingsProvider>
          <JobsProvider>
            <TooltipProvider>
              <GameSettingsCard game={game} root={ROOT} />
            </TooltipProvider>
          </JobsProvider>
        </SettingsProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe("game settings (CFG-2, VMC-1)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("plans a mode change and keeps bits the loader does not show", async () => {
    const fake = core([]);
    show(fake.transport, false);
    const mode2 = await screen.findByRole("checkbox", { name: /Mode 2/ });
    expect(screen.getByRole("checkbox", { name: /Mode 1/ })).toBeChecked();
    expect(screen.getByTestId("cfg-other-bits")).toHaveTextContent("0x40");
    const save = screen.getByTestId("cfg-save");
    expect(save).toBeDisabled();

    await userEvent.click(mode2);
    expect(save).toBeEnabled();
    await userEvent.click(save);
    const dialog = await screen.findByTestId("plan-dialog");
    expect(within(dialog).getByTestId("plan-cfg-changes")).toHaveTextContent("$Compatibility: 65 → 67");
    expect(fake.written).toHaveLength(1);
    expect(fake.written[0]!.settings?.compatibility).toBe(0x43);
    expect(fake.written[0]!.cards).toBeUndefined();
  });

  it("creates the slot's card in simple mode, replacing a file OPL refuses", async () => {
    const refused: VmcCard = {
      name: "SLUS_200.62_0",
      path: "E:\\VMC\\SLUS_200.62_0.bin",
      size: 65536,
      sizeMb: null,
      problem: "not a PS2 memory card image (no superblock magic)",
      problemPhrase: { key: "reasons.vmcBadMagic", params: {} },
      saves: null,
      usedBy: [],
    };
    const fake = core([refused]);
    show(fake.transport, true);
    await userEvent.click(await screen.findByTestId("cfg-vmc-create-0"));
    expect(screen.queryByTestId("cfg-vmc-1")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("cfg-save"));
    await waitFor(() => expect(fake.written).toHaveLength(1));
    expect(fake.written[0]!.settings?.vmc).toEqual(["SLUS_200.62_0", ""]);
    expect(fake.written[0]!.cards).toEqual([{ name: "SLUS_200.62_0", sizeMb: 8, replace: true }]);
  });

  it("takes a negative GSM shift and holds back one OPL's menu cannot set", async () => {
    const fake = core([]);
    show(fake.transport, false);
    const gsm = await screen.findByTestId("cfg-gsm");
    await userEvent.click(within(gsm).getByRole("radio", { name: /game.s own/ }));
    const shift = within(gsm).getByLabelText("Horizontal shift");
    await userEvent.clear(shift);
    await userEvent.type(shift, "-5");
    expect(shift).toHaveValue("-5");
    expect(screen.queryByTestId("cfg-offset-range")).not.toBeInTheDocument();

    await userEvent.clear(shift);
    await userEvent.type(shift, "999");
    expect(screen.getByTestId("cfg-offset-range")).toHaveTextContent("−100 to 100");
    expect(screen.getByTestId("cfg-save")).toBeDisabled();

    await userEvent.clear(shift);
    await userEvent.type(shift, "100");
    expect(screen.queryByTestId("cfg-offset-range")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("cfg-save"));
    await waitFor(() => expect(fake.written).toHaveLength(1));
    expect(fake.written[0]!.settings?.gsm).toMatchObject({ perGame: true, xOffset: 100 });
  });

  it("says why a new card's name is refused", async () => {
    show(core([]).transport, false);
    await userEvent.click(await screen.findByTestId("cfg-vmc-create-1"));
    const dialog = await screen.findByRole("dialog");
    const name = within(dialog).getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "bad/name");
    expect(within(dialog).getByTestId("cfg-card-name-invalid")).toBeInTheDocument();
    expect(within(dialog).getByTestId("cfg-vmc-new")).toBeDisabled();
    await userEvent.clear(name);
    await userEvent.type(name, "generic_1");
    expect(within(dialog).queryByTestId("cfg-card-name-invalid")).not.toBeInTheDocument();
    expect(within(dialog).getByTestId("cfg-vmc-new")).toBeEnabled();
  });

  it("offers no new card when a good one has the name", async () => {
    const good: VmcCard = {
      name: "SLUS_200.62_0",
      path: "E:\\VMC\\SLUS_200.62_0.bin",
      size: 8 << 20,
      sizeMb: 8,
      problem: null,
      saves: 2,
      usedBy: [{ startup: "SLUS_200.63", slot: 0 }],
    };
    show(core([good]).transport, true);
    expect(await screen.findByTestId("cfg-vmc-create-0")).toBeDisabled();
    expect(screen.getByText("SLUS_200.62_0 is already on the device: choose it in the list.")).toBeInTheDocument();
  });
});
