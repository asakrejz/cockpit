// Git in the cloned repo (/work). The runner only creates the commit – nothing is ever pushed in the container
// (it has no credentials for that); the backend does that after its check.

import { execFile } from "node:child_process";

export type Run = (args: string[], cwd: string) => Promise<string>;

export const gitRun: Run = (args, cwd) =>
  new Promise((ok, error) => {
    execFile(
      "git",
      args,
      { cwd, maxBuffer: 8 * 1024 * 1024, env: { ...process.env, GIT_TERMINAL_PROMPT: "0" } },
      (e, out, err) => (e ? error(new Error(`git ${args[0]}: ${(err || e.message).trim()}`)) : ok(out)),
    );
  });

export interface Recorded {
  commit: string;
  files: string[];
}

/** Makes sure the branch of the job is checked out and returns the changed files. */
export async function changedFiles(git: Run, cwd: string): Promise<string[]> {
  const out = await git(["status", "--porcelain", "-z", "--no-renames", "--untracked-files=all"], cwd);
  return out
    .split("\0")
    .filter((z) => z.length > 3)
    .map((z) => z.slice(3));
}

/** Creates a commit with all changes. `null` if there is nothing to commit. */
export async function record(
  git: Run,
  cwd: string,
  branch: string,
  message: string,
): Promise<Recorded | null> {
  const current = (await git(["rev-parse", "--abbrev-ref", "HEAD"], cwd)).trim();
  if (current !== branch) {
    throw new Error(`Der Läufer steht auf „${current}“, erwartet war „${branch}“.`);
  }
  const files = await changedFiles(git, cwd);
  if (files.length === 0) return null;
  await git(["add", "-A"], cwd);
  await git(
    [
      "-c", "user.name=Cockpit Agent",
      "-c", "user.email=agent@cockpit.invalid",
      "commit", "--no-verify", "-m", message,
    ],
    cwd,
  );
  const commit = (await git(["rev-parse", "HEAD"], cwd)).trim();
  return { commit, files };
}
