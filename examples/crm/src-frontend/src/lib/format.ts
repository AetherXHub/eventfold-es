import type { DealStage } from "@/api/types";
import { STAGE_DISPLAY_NAME } from "@/lib/constants";

/** Format cents as currency string (e.g., 5000 -> "$50.00"). */
export function formatCurrency(cents: number): string {
    return (
        "$" +
        (cents / 100).toLocaleString("en-US", {
            minimumFractionDigits: 2,
            maximumFractionDigits: 2,
        })
    );
}

/** Human-readable stage name. */
export function formatStageName(stage: string): string {
    if (stage in STAGE_DISPLAY_NAME) {
        return STAGE_DISPLAY_NAME[stage as DealStage];
    }
    return stage;
}

/** Truncate a UUID to its first 8-char segment for display. */
export function shortId(id: string): string {
    const dash = id.indexOf("-");
    return dash > 0 ? id.substring(0, dash) : id.substring(0, 8);
}

/**
 * Human-readable label for an event type.
 * Converts PascalCase to lowercase words (e.g., "StageAdvanced" -> "stage advanced").
 */
export function formatEventType(eventType: string): string {
    return eventType
        .replace(/([A-Z])/g, " $1")
        .trim()
        .toLowerCase();
}
