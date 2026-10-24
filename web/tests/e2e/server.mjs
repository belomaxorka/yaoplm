// Starts yaoplm-server for the e2e tests on a fresh fixture library
// (TESTING.md): `make_library` writes the device root, the import sources
// and the cover mirrors into a temp folder; all three become server roots.
// The art cache goes to the same temp folder. Playwright runs this as its
// webServer and kills the whole tree afterwards.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
// `node server.mjs [port] [--games N] [--root DIR]...`; 7878 serves the Vite
// dev server's /api proxy. `--games` grows the fixture library (virtualization
// checks); `--root` adds a folder of real test data as one more server root.
const args = process.argv.slice(2);
const option = (name) => args.flatMap((arg, i) => (arg === name && args[i + 1] ? [args[i + 1]] : []));
const port = args.find((arg, i) => !arg.startsWith("--") && !args[i - 1]?.startsWith("--")) ?? process.env.YAOPLM_E2E_PORT ?? "7879";
const games = option("--games").at(-1);
const extraRoots = option("--root");
const dir = mkdtempSync(path.join(tmpdir(), "yaoplm-e2e-"));

const made = spawnSync("cargo", ["run", "-q", "-p", "yaoplm-api", "--example", "make_library", "--", dir, ...(games ? ["--games", games] : [])], {
  cwd: repo,
  stdio: "inherit",
});
if (made.status !== 0) {
  rmSync(dir, { recursive: true, force: true });
  process.exit(made.status ?? 1);
}

const server = spawn(
  "cargo",
  [
    "run",
    "-q",
    "-p",
    "yaoplm-server",
    "--",
    "--bind",
    `127.0.0.1:${port}`,
    "--root",
    path.join(dir, "library"),
    "--root",
    path.join(dir, "sources"),
    "--root",
    path.join(dir, "mirror"),
    "--data-dir",
    path.join(dir, "data"),
    ...extraRoots.flatMap((root) => ["--root", root]),
  ],
  // A source the settings forget to point at a mirror fails instead of
  // going to the network.
  { cwd: repo, stdio: "inherit", env: { ...process.env, YAOPLM_OFFLINE: "1" } },
);

const stop = () => server.kill();
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
server.on("exit", (code) => {
  rmSync(dir, { recursive: true, force: true });
  process.exit(code ?? 0);
});
