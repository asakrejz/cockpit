// Git in the cloned repo (/work). The runner only creates the commit – nothing is ever pushed in the container
// (it has no credentials for that); the backend does that after its check.

import { execFile } from "node:child_process";
import { join } from "node:path";

import { type Job, commitMessage } from "./job.ts";

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
    throw new Error(`The runner is on “${current}”, expected “${branch}”.`);
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

/** What was committed in one repo of a job in several repos. */
export interface RepoCommit {
  repo: string;
  branch: string;
  commit: string;
}

/** What `recordJob` committed: the changed files (in several repos with the repo folder in front), the commit
 * of the first changed repo, and the repos that have a commit. */
export interface JobRecord {
  files: string[];
  commit: string;
  repos: RepoCommit[];
}

/** The folders of the repos of a job below `root`: one entry with an empty folder for a job in one repo. */
export function repoDirs(job: Job, root: string): Array<{ dir: string; cwd: string; branch: string; repo: string; pr_language: Job["pr_language"] }> {
  if (job.repos.length > 1) {
    return job.repos.map((r) => ({ dir: r.dir, cwd: join(root, r.dir), branch: r.branch, repo: r.repo, pr_language: r.pr_language }));
  }
  return [{ dir: "", cwd: root, branch: job.branch, repo: job.repo, pr_language: job.pr_language }];
}

/** `dir/file` for a job in several repos, `file` for one. */
function inRoot(dir: string, file: string): string {
  return dir ? `${dir}/${file}` : file;
}

/** The commit each repo stands on now (by folder), to find out later what moved. */
export async function headsOf(git: Run, job: Job, root: string): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const r of repoDirs(job, root)) {
    try {
      out[r.dir] = (await git(["rev-parse", "HEAD"], r.cwd)).trim();
    } catch {
      out[r.dir] = "";
    }
  }
  return out;
}

/** Every file that differs from the start in any repo: working tree and commits since `heads`. */
export async function touchedFiles(git: Run, job: Job, root: string, heads: Record<string, string>): Promise<string[]> {
  const all: string[] = [];
  for (const r of repoDirs(job, root)) {
    let files = await changedFiles(git, r.cwd);
    const head = heads[r.dir] ?? "";
    const now = head ? (await git(["rev-parse", "HEAD"], r.cwd)).trim() : head;
    if (head && now !== head) {
      const moved = await git(["diff", "--name-only", "-z", `${head}..${now}`], r.cwd);
      files = [...new Set([...files, ...moved.split("\0").filter(Boolean)])];
    }
    all.push(...files.map((f) => inRoot(r.dir, f)));
  }
  return all;
}

/** Commits the changes of every repo of the job on its own branch. Repos without changes are left alone.
 * `null` if nothing changed anywhere. */
export async function recordJob(git: Run, root: string, job: Job): Promise<JobRecord | null> {
  const files: string[] = [];
  const repos: RepoCommit[] = [];
  const multi = job.repos.length > 1;
  for (const r of repoDirs(job, root)) {
    const f = await record(git, r.cwd, r.branch, commitMessage(job, multi ? r.pr_language : undefined));
    if (!f) continue;
    files.push(...f.files.map((x) => inRoot(r.dir, x)));
    repos.push({ repo: r.repo, branch: r.branch, commit: f.commit });
  }
  if (repos.length === 0) return null;
  return { files, commit: repos[0].commit, repos };
}
