import { isTauri } from "./bridge";
import type { Language } from "../types";

export const RELEASE_URL = "https://github.com/komakizhu/Quota-Pro/releases/latest";
export const PROJECT_URL = "https://github.com/komakizhu/Quota-Pro";

export interface UpdateMessages {
  checking: string;
  current: string;
  downloading: (version: string) => string;
  installing: string;
  availableWindows: (version: string) => string;
  availableMac: (version: string) => string;
  failed: string;
}

export async function openReleasePage(): Promise<void> {
  if (!isTauri()) {
    window.open(RELEASE_URL, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(RELEASE_URL);
}

export async function openProjectPage(): Promise<void> {
  if (!isTauri()) {
    window.open(PROJECT_URL, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(PROJECT_URL);
}

export async function checkForAppUpdate(
  language: Language,
  messages: UpdateMessages,
  setStatus: (message: string | null) => void,
  manual = false,
): Promise<void> {
  if (!isTauri()) {
    if (manual) setStatus(messages.current);
    return;
  }

  if (manual) setStatus(messages.checking);
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check({ timeout: 15_000 });
    if (!update) {
      if (manual) setStatus(messages.current);
      return;
    }

    const isMac = /Macintosh|Mac OS X/i.test(navigator.userAgent);
    if (isMac) {
      const prompt = messages.availableMac(update.version);
      setStatus(prompt);
      if (window.confirm(prompt)) {
        await openReleasePage();
      }
      return;
    }

    const prompt = messages.availableWindows(update.version);
    setStatus(prompt);
    if (!window.confirm(prompt)) return;

    setStatus(messages.downloading(update.version));
    await update.downloadAndInstall((event) => {
      if (event.event === "Finished") setStatus(messages.installing);
    });
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  } catch {
    if (!manual) return;
    setStatus(messages.failed);
  }
}
