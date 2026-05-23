// updater.ts
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { toast, type ExternalToast } from "svelte-sonner";
import Logger from "$utils/log";

type ToastId = string | number;

export type UpdateToastCopy = {
    updateAvailable: (version: string) => string;
    releaseNotes: string;
    dismiss: string;
    installPrompt: (version: string) => string;
    update: string;
    later: string;
    downloading: string;
    downloadingProgress: (percent: number) => string;
    installing: string;
    restarting: string;
    installFailed: (error: string) => string;
};

export type CheckForAppUpdatesOptions = {
    copy?: Partial<UpdateToastCopy>;
    durationMs?: number;
    openReleaseNotes?: (url: string) => Promise<void>;
};

const UPDATE_TOAST_ID = "app-update-available";
const DEFAULT_TOAST_DURATION_MS = 12000;

const DEFAULT_COPY: UpdateToastCopy = {
    updateAvailable: (version) => `Update ${version} available`,
    releaseNotes: "Release notes",
    dismiss: "Dismiss",
    installPrompt: (version) => `Ready to update to ${version}`,
    update: "Update",
    later: "Later",
    downloading: "Downloading update...",
    downloadingProgress: (percent) => `Downloading update... ${percent}%`,
    installing: "Installing update...",
    restarting: "Restarting...",
    installFailed: (error) => `Update failed: ${error}`,
};

function resolveCopy(copy?: Partial<UpdateToastCopy>): UpdateToastCopy {
    return {
        updateAvailable: copy?.updateAvailable ?? DEFAULT_COPY.updateAvailable,
        releaseNotes: copy?.releaseNotes ?? DEFAULT_COPY.releaseNotes,
        dismiss: copy?.dismiss ?? DEFAULT_COPY.dismiss,
        installPrompt: copy?.installPrompt ?? DEFAULT_COPY.installPrompt,
        update: copy?.update ?? DEFAULT_COPY.update,
        later: copy?.later ?? DEFAULT_COPY.later,
        downloading: copy?.downloading ?? DEFAULT_COPY.downloading,
        downloadingProgress:
            copy?.downloadingProgress ?? DEFAULT_COPY.downloadingProgress,
        installing: copy?.installing ?? DEFAULT_COPY.installing,
        restarting: copy?.restarting ?? DEFAULT_COPY.restarting,
        installFailed: copy?.installFailed ?? DEFAULT_COPY.installFailed,
    };
}

function formatError(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
}

function loadingToastOptions(id: ToastId): ExternalToast {
    return {
        id,
        duration: Number.POSITIVE_INFINITY,
        dismissable: false,
    };
}

function downloadProgressPercent(
    downloadedBytes: number,
    contentLength?: number,
): number | undefined {
    if (!contentLength || contentLength <= 0) return undefined;
    return Math.min(
        100,
        Math.max(0, Math.floor((downloadedBytes / contentLength) * 100)),
    );
}

export async function checkForAppUpdates(
    release_url: string,
    options: CheckForAppUpdatesOptions = {},
) {
    const copy = resolveCopy(options.copy);
    const duration = options.durationMs ?? DEFAULT_TOAST_DURATION_MS;
    const openReleaseNotes = options.openReleaseNotes ?? openUrl;
    const update = await check();

    if (!update) {
        Logger.info("No update available");
        return;
    }

    Logger.info("Update available!", update);

    const toastId = UPDATE_TOAST_ID;

    const showInstallPrompt = async () => {
        try {
            await openReleaseNotes(release_url);
        } catch (error) {
            Logger.error("Failed to open release notes", error);
        }

        toast.info(copy.installPrompt(update.version), {
            id: toastId,
            duration,
            action: {
                label: copy.update,
                onClick: async (event) => {
                    event.preventDefault();
                    await installUpdate(
                        toastId,
                        copy,
                        update.downloadAndInstall.bind(update),
                    );
                },
            },
            cancel: {
                label: copy.later,
            },
        });
    };

    toast.info(copy.updateAvailable(update.version), {
        id: toastId,
        duration,
        action: {
            label: copy.releaseNotes,
            onClick: async (event) => {
                event.preventDefault();
                await showInstallPrompt();
            },
        },
        cancel: {
            label: copy.dismiss,
        },
    });
}

async function installUpdate(
    toastId: ToastId,
    copy: UpdateToastCopy,
    downloadAndInstall: (
        onEvent?: (event: DownloadEvent) => void,
    ) => Promise<void>,
) {
    let contentLength: number | undefined;
    let downloadedBytes = 0;
    let lastPercent: number | undefined;

    toast.loading(copy.downloading, loadingToastOptions(toastId));

    try {
        await downloadAndInstall((event) => {
            if (event.event === "Started") {
                contentLength = event.data.contentLength;
                downloadedBytes = 0;
                lastPercent = undefined;
                toast.loading(copy.downloading, loadingToastOptions(toastId));
                return;
            }

            if (event.event === "Progress") {
                downloadedBytes += event.data.chunkLength;
                const percent = downloadProgressPercent(
                    downloadedBytes,
                    contentLength,
                );

                if (percent !== undefined && percent !== lastPercent) {
                    lastPercent = percent;
                    toast.loading(
                        copy.downloadingProgress(percent),
                        loadingToastOptions(toastId),
                    );
                }

                return;
            }

            toast.loading(copy.installing, loadingToastOptions(toastId));
        });

        toast.loading(copy.restarting, loadingToastOptions(toastId));
        await relaunch();
    } catch (error) {
        Logger.error("Update install failed", error);
        toast.error(copy.installFailed(formatError(error)), {
            id: toastId,
            duration: 6000,
        });
    }
}
