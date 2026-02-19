import type { DealStage } from "@/api/types";

/** Pipeline stages in display order. */
export const DEAL_STAGES: readonly DealStage[] = [
    "Prospect",
    "Qualified",
    "Proposal",
    "Negotiation",
    "ClosedWon",
    "ClosedLost",
] as const;

/** Human-readable stage names for column headers. */
export const STAGE_DISPLAY_NAME: Record<DealStage, string> = {
    Prospect: "Prospect",
    Qualified: "Qualified",
    Proposal: "Proposal",
    Negotiation: "Negotiation",
    ClosedWon: "Won",
    ClosedLost: "Lost",
};

/** Tailwind background class per deal stage for badges. */
export const STAGE_BADGE_CLASS: Record<DealStage, string> = {
    Prospect: "bg-indigo-500",
    Qualified: "bg-violet-500",
    Proposal: "bg-amber-500",
    Negotiation: "bg-orange-500",
    ClosedWon: "bg-green-500",
    ClosedLost: "bg-red-500",
};

/** Tailwind background class per aggregate type for activity feed badges. */
export const AGGREGATE_BADGE_CLASS: Record<string, string> = {
    contact: "bg-indigo-500",
    company: "bg-violet-500",
    deal: "bg-amber-500",
    task: "bg-orange-500",
    note: "bg-slate-500",
};

/** View route per aggregate type for activity feed navigation. */
export const AGGREGATE_ROUTE: Record<string, string> = {
    contact: "/contacts",
    company: "/companies",
    deal: "/deals",
    task: "/tasks",
    note: "/notes",
};
