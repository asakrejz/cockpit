// Git im geklonten Repo (/work). Der Läufer legt nur den Commit an – gepusht wird nie im Container
// (er hat keine Zugangsdaten dafür); das macht das Backend nach seiner Prüfung.

import { execFile } from "node:child_process";

export type Ausfuehren = (args: string[], cwd: string) => Promise<string>;

export const gitAusfuehren: Ausfuehren = (args, cwd) =>
  new Promise((ok, fehler) => {
    execFile(
      "git",
      args,
      { cwd, maxBuffer: 8 * 1024 * 1024, env: { ...process.env, GIT_TERMINAL_PROMPT: "0" } },
      (e, out, err) => (e ? fehler(new Error(`git ${args[0]}: ${(err || e.message).trim()}`)) : ok(out)),
    );
  });

export interface Festgehalten {
  commit: string;
  dateien: string[];
}

/** Stellt sicher, dass der Branch des Auftrags ausgecheckt ist, und liefert die geänderten Dateien. */
export async function geaenderteDateien(git: Ausfuehren, cwd: string): Promise<string[]> {
  const out = await git(["status", "--porcelain", "-z", "--no-renames", "--untracked-files=all"], cwd);
  return out
    .split("\0")
    .filter((z) => z.length > 3)
    .map((z) => z.slice(3));
}

/** Legt einen Commit mit allen Änderungen an. `null`, wenn es nichts zu committen gibt. */
export async function festhalten(
  git: Ausfuehren,
  cwd: string,
  branch: string,
  nachricht: string,
): Promise<Festgehalten | null> {
  const aktuell = (await git(["rev-parse", "--abbrev-ref", "HEAD"], cwd)).trim();
  if (aktuell !== branch) {
    throw new Error(`Der Läufer steht auf „${aktuell}“, erwartet war „${branch}“.`);
  }
  const dateien = await geaenderteDateien(git, cwd);
  if (dateien.length === 0) return null;
  await git(["add", "-A"], cwd);
  await git(
    [
      "-c", "user.name=Cockpit Agent",
      "-c", "user.email=agent@cockpit.invalid",
      "commit", "--no-verify", "-m", nachricht,
    ],
    cwd,
  );
  const commit = (await git(["rev-parse", "HEAD"], cwd)).trim();
  return { commit, dateien };
}
