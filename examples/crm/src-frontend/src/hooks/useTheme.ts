import { useCallback, useEffect } from "react";
import { useSettings } from "@/api/queries";
import { useSetDarkMode } from "@/api/mutations";

const STORAGE_KEY = "efcrm-theme";

/**
 * Apply the `dark` class to `<html>` and sync localStorage.
 * Called both for optimistic updates and when the backend query resolves.
 */
function applyTheme(dark: boolean): void {
    document.documentElement.classList.toggle("dark", dark);
    localStorage.setItem(STORAGE_KEY, dark ? "dark" : "light");
}

// Apply localStorage value synchronously at module load to avoid a flash of
// the wrong theme before the backend query resolves.
applyTheme(localStorage.getItem(STORAGE_KEY) === "dark");

/**
 * Hook for managing light/dark theme.
 *
 * Uses the event store (via `get_settings` / `set_dark_mode`) as the source
 * of truth, with localStorage as a synchronous fallback for initial render.
 */
export function useTheme(): { isDark: boolean; toggle: () => void } {
    const { data: settings } = useSettings();
    const mutation = useSetDarkMode();

    // Sync DOM whenever the backend settings change.
    useEffect(() => {
        if (settings !== undefined) {
            applyTheme(settings.dark_mode);
        }
    }, [settings]);

    // Derive current state: backend value if loaded, else localStorage fallback.
    const isDark =
        settings !== undefined ? settings.dark_mode : localStorage.getItem(STORAGE_KEY) === "dark";

    const toggle = useCallback(() => {
        const next = !document.documentElement.classList.contains("dark");
        // Optimistic: flip DOM + localStorage immediately.
        applyTheme(next);
        // Persist to the event store.
        mutation.mutate({ enabled: next });
    }, [mutation]);

    return { isDark, toggle };
}
