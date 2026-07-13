import { getSetting, setSetting } from "./rpc";

const LAUNCH_COUNT_KEY = "app_launch_count";
const NEXT_PROMPT_LAUNCH_KEY = "feedback_next_prompt_launch";
const DISMISSED_KEY = "feedback_prompt_dismissed";
const FIRST_PROMPT_LAUNCH = 5;
const REMINDER_INTERVAL = 10;

export const FEEDBACK_SURVEY_URL = "https://tally.so/r/EkEWVo";
export const FEEDBACK_EMAIL_URL = "mailto:gist@jthiepler.com?subject=Gist%20feedback";
export const FEEDBACK_GITHUB_URL = "https://github.com/jthiepler/gist/issues";

export interface FeedbackLaunchState {
  launchCount: number;
  shouldPrompt: boolean;
}

function parseStoredNumber(value: string | null, fallback: number): number {
  const parsed = Number.parseInt(value ?? "", 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

export async function recordAppLaunch(): Promise<FeedbackLaunchState> {
  const [storedCount, dismissed, nextPromptLaunch] = await Promise.all([
    getSetting(LAUNCH_COUNT_KEY),
    getSetting(DISMISSED_KEY),
    getSetting(NEXT_PROMPT_LAUNCH_KEY),
  ]);
  const launchCount = parseStoredNumber(storedCount, 0) + 1;

  await setSetting(LAUNCH_COUNT_KEY, String(launchCount));

  return {
    launchCount,
    shouldPrompt:
      dismissed !== "true" &&
      launchCount >= parseStoredNumber(nextPromptLaunch, FIRST_PROMPT_LAUNCH),
  };
}

export async function deferFeedbackPrompt(launchCount: number): Promise<void> {
  await setSetting(NEXT_PROMPT_LAUNCH_KEY, String(launchCount + REMINDER_INTERVAL));
}

export async function dismissFeedbackPrompt(): Promise<void> {
  await setSetting(DISMISSED_KEY, "true");
}
