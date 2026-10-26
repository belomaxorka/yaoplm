import { test } from "@playwright/test";

import { emulatorFlow } from "./emulator-flow";

// A memory card emulator's SD in advanced mode (ADR-0048); simple mode runs
// the same flow in `simple-mode.spec.ts`.

test("advanced mode: the emulator's SD is known by its files and gets OPL's folders by its boot cards", async ({
  page,
  request,
}) => {
  await emulatorFlow(page, request, false);
});
