import { useQuery } from "@tanstack/react-query";
import { invoke } from "@/api/invoke";
import type {
    ActivityFeed,
    AppSettings,
    CompanyWithId,
    ContactWithId,
    DealWithId,
    NoteWithId,
    PipelineSummary,
    TaskWithId,
} from "@/api/types";

export function useContacts() {
    return useQuery({
        queryKey: ["contacts"],
        queryFn: () => invoke<ContactWithId[]>("list_contacts"),
    });
}

export function useContact(id: string) {
    return useQuery({
        queryKey: ["contacts", id],
        queryFn: () => invoke<ContactWithId>("get_contact", { id }),
    });
}

export function useCompanies() {
    return useQuery({
        queryKey: ["companies"],
        queryFn: () => invoke<CompanyWithId[]>("list_companies"),
    });
}

export function useCompany(id: string) {
    return useQuery({
        queryKey: ["companies", id],
        queryFn: () => invoke<CompanyWithId>("get_company", { id }),
    });
}

export function useDeals() {
    return useQuery({
        queryKey: ["deals"],
        queryFn: () => invoke<DealWithId[]>("list_deals"),
    });
}

export function useDeal(id: string) {
    return useQuery({
        queryKey: ["deals", id],
        queryFn: () => invoke<DealWithId>("get_deal", { id }),
    });
}

export function useTasks() {
    return useQuery({
        queryKey: ["tasks"],
        queryFn: () => invoke<TaskWithId[]>("list_tasks"),
    });
}

export function useNotes() {
    return useQuery({
        queryKey: ["notes"],
        queryFn: () => invoke<NoteWithId[]>("list_notes"),
    });
}

export function usePipelineSummary() {
    return useQuery({
        queryKey: ["pipeline"],
        queryFn: () => invoke<PipelineSummary>("get_pipeline_summary"),
    });
}

export function useActivityFeed() {
    return useQuery({
        queryKey: ["activity"],
        queryFn: () => invoke<ActivityFeed>("get_activity_feed"),
    });
}

export function useSettings() {
    return useQuery({
        queryKey: ["settings"],
        queryFn: () => invoke<AppSettings>("get_settings"),
    });
}
