// Texts that the runner writes for people (step titles, errors, the commit message). The backend
// passes the language of the interface in the job (`language`) and the language of the pull request
// text (`pr_language`); the runner works for one job per process, so the language is set once.

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
  answer_language: { de: "Antworte auf Deutsch.", en: "Answer in English." },
  review_language: {
    de: "Schreibe die Zusammenfassung und alle Review-Kommentare auf Deutsch, der Sprache des Pull Requests.",
    en: "Write the summary and all review comments in English, the language of the pull request.",
  },
  commit_followup_comments: { de: "Review-Kommentare umsetzen (#{pr})", en: "Address review comments (#{pr})" },
  commit_followup_checks: { de: "Rote Checks beheben (#{pr})", en: "Fix failing checks (#{pr})" },
  commit_footer: { de: "Umgesetzt vom Cockpit-Agenten.", en: "Implemented by the Cockpit agent." },
} satisfies Table;

export type TextId = keyof typeof TEXTS;

let current: Lang = "de";

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
