// Texts that the runner writes for people (step titles, errors, deny messages, the result line of a
// review, the commit message). The backend passes the language of the interface in the job (`language`)
// and the language of the pull request text (`pr_language`); the runner works for one job per process,
// so the language is set once. The prompts for the model are not here: they are English (job.ts).

export type Lang = "de" | "en";

/** Reads a language code from the job; anything unknown falls back to `fallback`. */
export function langOf(w: unknown, fallback: Lang): Lang {
  return w === "de" || w === "en" ? w : fallback;
}

type Table = Record<string, { de: string; en: string }>;

const TEXTS = {
  step_read: { de: "Datei lesen", en: "Read file" },
  step_write: { de: "Datei schreiben", en: "Write file" },
  step_change: { de: "Datei ändern", en: "Change file" },
  step_search: { de: "Code durchsuchen", en: "Search code" },
  step_run: { de: "Befehl ausführen", en: "Run command" },
  step_web: { de: "Websuche", en: "Web search" },
  step_tool: { de: "Werkzeug", en: "Tool" },
  step_think: { de: "Überlegt", en: "Thinking" },
  error_unexpected: { de: "Unerwarteter Fehler im Läufer: {message}", en: "Unexpected error in the runner: {message}" },
  error_timeout: {
    de: "Das Zeitlimit von {minutes} Minuten im Container ist überschritten.",
    en: "The time limit of {minutes} minutes in the container has been exceeded.",
  },
  error_sdk: { de: "Das SDK meldet: {message}", en: "The SDK reports: {message}" },
  error_sdk_load: {
    de: "Das Agent SDK lässt sich nicht laden ({message}). Image neu bauen?",
    en: "The Agent SDK cannot be loaded ({message}). Rebuild the image?",
  },
  error_nothing_changed: {
    de: "Der Agent hat nichts geändert, es gibt nichts zu pushen.",
    en: "The agent changed nothing, so there is nothing to push.",
  },
  error_model_ended: {
    de: "Das Modell hat den Lauf mit „{reason}“ beendet.",
    en: "The model ended the run with “{reason}”.",
  },
  error_model_failed: {
    de: "Das Modell hat den Lauf mit einem Fehler beendet.",
    en: "The model ended the run with an error.",
  },
  error_cli_missing: {
    de: "{tool} ist im Image nicht vorhanden. Image neu bauen?",
    en: "{tool} is not in the image. Rebuild the image?",
  },
  error_cli_no_result: {
    de: "{tool} hat den Lauf ohne Ergebnis beendet{broken}.{tail}",
    en: "{tool} ended the run without a result{broken}.{tail}",
  },
  error_cli_status: {
    de: "{tool} hat den Lauf mit Status {code} beendet{broken}.{tail}",
    en: "{tool} ended the run with status {code}{broken}.{tail}",
  },
  error_cli_exit: { de: "{tool} hat sich mit Status {code} beendet.", en: "{tool} exited with status {code}." },
  broken_lines: { de: " ({n} unlesbare Zeilen)", en: " ({n} unreadable lines)" },
  error_plan_changed: {
    de: "{tool} hat im Planmodus Dateien verändert ({files}). Der Plan wird verworfen.",
    en: "{tool} changed files in plan mode ({files}). The plan is discarded.",
  },
  error_review_changed: {
    de: "{tool} hat im Review Dateien verändert ({files}). Das Review wird verworfen.",
    en: "{tool} changed files in the review ({files}). The review is discarded.",
  },
  error_rules: {
    de: "{tool} hat gegen die Regeln verstoßen, nichts wird übernommen: {list}",
    en: "{tool} broke the rules, nothing is kept: {list}",
  },
  result_done: { de: "{key} umgesetzt.", en: "{key} done." },
  review_nothing: { de: "Nichts zu beanstanden.", en: "Nothing to flag." },
  review_drafted_one: { de: "1 Kommentar als Entwurf vorbereitet.", en: "1 comment prepared as a draft." },
  review_drafted_many: { de: "{n} Kommentare als Entwurf vorbereitet.", en: "{n} comments prepared as drafts." },
  // The prompts are English; only these two sentences name the language the model writes in
  // (so both entries are English text, whatever the job language is).
  answer_language: { de: "Write your answer in German.", en: "Write your answer in English." },
  review_language: {
    de: "Write the summary and all review comments in German, the language of the pull request.",
    en: "Write the summary and all review comments in English, the language of the pull request.",
  },
  deny_outside: { de: "„{path}“ liegt außerhalb von {root}.", en: "“{path}” is outside {root}." },
  deny_no_repo: {
    de: "„{path}“ liegt in keinem der Repos dieses Auftrags.",
    en: "“{path}” is in none of the repos of this job.",
  },
  deny_git_dir: {
    de: "„{path}“: Das .git-Verzeichnis darf nicht verändert werden.",
    en: "“{path}”: The .git directory must not be changed.",
  },
  deny_forbidden: { de: "„{path}“ ist laut Regel „{rule}“ verboten.", en: "“{path}” is forbidden by the rule “{rule}”." },
  deny_not_allowed: {
    de: "„{path}“ liegt außerhalb der erlaubten Pfade ({allowed}).",
    en: "“{path}” is outside the allowed paths ({allowed}).",
  },
  deny_tool: {
    de: "Das Werkzeug „{tool}“ ist im Cockpit nicht freigegeben.",
    en: "The tool “{tool}” is not enabled in Cockpit.",
  },
  deny_command_git: {
    de: "Git-Befehle, die ins Netz gehen oder den Verlauf umschreiben, sind nicht erlaubt.",
    en: "Git commands that use the network or rewrite history are not allowed.",
  },
  deny_command_main: { de: "Auf den Hauptbranch wechselt der Agent nie.", en: "The agent never switches to the main branch." },
  deny_command_network: { de: "Netzwerkzugriffe sind nicht erlaubt.", en: "Network access is not allowed." },
  deny_command_rights: { de: "Rechte ändert der Agent nicht.", en: "The agent does not change permissions." },
  deny_command_rm: {
    de: "Aufräumen außerhalb von /work ist nicht erlaubt.",
    en: "Cleaning up outside /work is not allowed.",
  },
  deny_command_git_dir: { de: "Das .git-Verzeichnis fasst der Agent nicht an.", en: "The agent does not touch the .git directory." },
  deny_command_secrets: { de: "Zugangsdaten liest der Agent nicht aus.", en: "The agent does not read credentials." },
  // Check mode (`check.ts`): does the container reach the model endpoint? `{who}` is "Anthropic", "OpenAI"
  // or the endpoint (subject form in the answers, object form in "cannot reach").
  check_no_endpoint: {
    de: "Im Container ist weder eine Endpunkt-Adresse noch ein Anthropic-Key eingestellt.",
    en: "The container has neither an endpoint address nor an Anthropic key set.",
  },
  check_no_openai_key: {
    de: "Im Container ist kein Key (OPENAI_API_KEY) eingestellt.",
    en: "The container has no key (OPENAI_API_KEY) set.",
  },
  check_answers: { de: "{who} antwortet aus dem Container.", en: "{who} answers from the container." },
  check_key_refused: {
    de: "{who} antwortet aus dem Container, lehnt den Key aber ab (Status {status}).",
    en: "{who} answers from the container but refuses the key (status {status}).",
  },
  check_unreachable: {
    de: "Der Container erreicht {who} nicht ({reason}).",
    en: "The container cannot reach {who} ({reason}).",
  },
  check_no_answer: { de: "keine Antwort innerhalb von 10 Sekunden", en: "no answer within 10 seconds" },
  check_endpoint_subject: { de: "Der Endpunkt", en: "The endpoint" },
  check_endpoint_object: { de: "den Endpunkt", en: "the endpoint" },
  commit_followup_comments: { de: "Review-Kommentare umsetzen (#{pr})", en: "Address review comments (#{pr})" },
  commit_followup_checks: { de: "Rote Checks beheben (#{pr})", en: "Fix failing checks (#{pr})" },
  commit_footer: { de: "Umgesetzt vom Cockpit-Agenten.", en: "Implemented by the Cockpit agent." },
} satisfies Table;

export type TextId = keyof typeof TEXTS;

let current: Lang = "en";

/** Sets the language of the user-facing texts for this process. */
export function setLanguage(lang: Lang): void {
  current = lang;
}

export function language(): Lang {
  return current;
}

/** The text `id` in `lang` (default: the language of the job) with `{name}` parameters filled in. */
export function tx(id: TextId, params: Record<string, string | number> = {}, lang: Lang = current): string {
  let text: string = TEXTS[id][lang];
  for (const [k, v] of Object.entries(params)) text = text.split(`{${k}}`).join(String(v));
  return text;
}
