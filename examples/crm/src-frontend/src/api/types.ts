export type DealStage =
    | "Prospect"
    | "Qualified"
    | "Proposal"
    | "Negotiation"
    | "ClosedWon"
    | "ClosedLost";

export interface ContactWithId {
    id: string;
    name: string;
    email: string;
    phone: string;
    company_id: string | null;
    tags: string[];
}

export interface CompanyWithId {
    id: string;
    name: string;
    industry: string;
    website: string | null;
}

export interface DealWithId {
    id: string;
    title: string;
    company_id: string;
    contact_id: string;
    stage: DealStage;
    value: number;
    closed: boolean;
}

export interface TaskWithId {
    id: string;
    title: string;
    description: string;
    deal_id: string | null;
    contact_id: string | null;
    done: boolean;
    due: string | null;
}

export interface NoteWithId {
    id: string;
    body: string;
    contact_id: string | null;
    deal_id: string | null;
    company_id: string | null;
    task_id: string | null;
}

export interface DealSummary {
    deal_id: string;
    title: string;
    stage: string;
    value: number;
}

export interface PipelineSummary {
    by_stage: Record<string, DealSummary[]>;
    total_value: number;
    won_value: number;
    lost_value: number;
}

export interface ActivityEntry {
    aggregate_type: string;
    instance_id: string;
    event_type: string;
    ts: number;
}

export interface ActivityFeed {
    entries: ActivityEntry[];
}

export interface AppSettings {
    dark_mode: boolean;
}
