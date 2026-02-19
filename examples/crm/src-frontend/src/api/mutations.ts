import { useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@/api/invoke";

export function useCreateContact() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string; name: string; email: string; phone: string }) =>
            invoke<void>("create_contact", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["contacts"] });
        },
    });
}

export function useArchiveContact() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("archive_contact", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["contacts"] });
        },
    });
}

export function useAddContactTag() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string; tag: string }) => invoke<void>("add_contact_tag", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["contacts"] });
        },
    });
}

export function useLinkContactCompany() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string; companyId: string }) =>
            invoke<void>("link_contact_company", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["contacts"] });
        },
    });
}

export function useCreateCompany() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: {
            id: string;
            name: string;
            industry: string;
            website: string | null;
        }) => invoke<void>("create_company", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["companies"] });
        },
    });
}

export function useArchiveCompany() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("archive_company", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["companies"] });
        },
    });
}

export function useCreateDeal() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: {
            id: string;
            title: string;
            companyId: string;
            contactId: string;
            value: number;
        }) => invoke<void>("create_deal", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["deals"] });
            void qc.invalidateQueries({ queryKey: ["pipeline"] });
            void qc.invalidateQueries({ queryKey: ["activity"] });
        },
    });
}

export function useAdvanceDeal() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("advance_deal", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["deals"] });
            void qc.invalidateQueries({ queryKey: ["pipeline"] });
            void qc.invalidateQueries({ queryKey: ["activity"] });
        },
    });
}

export function useWinDeal() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("win_deal", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["deals"] });
            void qc.invalidateQueries({ queryKey: ["pipeline"] });
            void qc.invalidateQueries({ queryKey: ["activity"] });
        },
    });
}

export function useLoseDeal() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("lose_deal", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["deals"] });
            void qc.invalidateQueries({ queryKey: ["pipeline"] });
            void qc.invalidateQueries({ queryKey: ["activity"] });
        },
    });
}

export function useCreateTask() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: {
            id: string;
            title: string;
            description: string;
            dealId: string | null;
            contactId: string | null;
            due: string | null;
        }) => invoke<void>("create_task", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["tasks"] });
        },
    });
}

export function useCompleteTask() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("complete_task", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["tasks"] });
        },
    });
}

export function useReopenTask() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { id: string }) => invoke<void>("reopen_task", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["tasks"] });
        },
    });
}

export function useSetDarkMode() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: { enabled: boolean }) => invoke<void>("set_dark_mode", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["settings"] });
        },
    });
}

export function useCreateNote() {
    const qc = useQueryClient();
    return useMutation({
        mutationFn: (args: {
            id: string;
            body: string;
            contactId: string | null;
            dealId: string | null;
            companyId: string | null;
            taskId: string | null;
        }) => invoke<void>("create_note", args),
        onSuccess: () => {
            void qc.invalidateQueries({ queryKey: ["notes"] });
            void qc.invalidateQueries({ queryKey: ["activity"] });
        },
    });
}
