import { useState } from "react";
import Modal from "@/components/shared/Modal";
import { useCreateNote } from "@/api/mutations";
import { useContacts, useDeals, useCompanies, useTasks } from "@/api/queries";

interface NoteFormProps {
    onClose: () => void;
    prefillContactId?: string | null;
    prefillDealId?: string | null;
    prefillCompanyId?: string | null;
    prefillTaskId?: string | null;
}

export default function NoteForm({
    onClose,
    prefillContactId,
    prefillDealId,
    prefillCompanyId,
    prefillTaskId,
}: NoteFormProps) {
    const { data: contacts = [] } = useContacts();
    const { data: deals = [] } = useDeals();
    const { data: companies = [] } = useCompanies();
    const { data: tasks = [] } = useTasks();
    const createNote = useCreateNote();

    const [body, setBody] = useState("");
    const [contactId, setContactId] = useState(prefillContactId ?? "");
    const [dealId, setDealId] = useState(prefillDealId ?? "");
    const [companyId, setCompanyId] = useState(prefillCompanyId ?? "");
    const [taskId, setTaskId] = useState(prefillTaskId ?? "");

    async function handleSubmit(e: React.SyntheticEvent) {
        e.preventDefault();
        await createNote.mutateAsync({
            id: crypto.randomUUID(),
            body,
            contactId: contactId || null,
            dealId: dealId || null,
            companyId: companyId || null,
            taskId: taskId || null,
        });
        onClose();
    }

    return (
        <Modal open title="New Note" onClose={onClose}>
            <form onSubmit={handleSubmit}>
                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Note
                </label>
                <textarea
                    required
                    value={body}
                    onChange={(e) => setBody(e.target.value)}
                    rows={4}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Contact
                </label>
                <select
                    value={contactId}
                    onChange={(e) => setContactId(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">None</option>
                    {contacts.map((c) => (
                        <option key={c.id} value={c.id}>
                            {c.name}
                        </option>
                    ))}
                </select>

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Deal
                </label>
                <select
                    value={dealId}
                    onChange={(e) => setDealId(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">None</option>
                    {deals.map((d) => (
                        <option key={d.id} value={d.id}>
                            {d.title}
                        </option>
                    ))}
                </select>

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Company
                </label>
                <select
                    value={companyId}
                    onChange={(e) => setCompanyId(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">None</option>
                    {companies.map((c) => (
                        <option key={c.id} value={c.id}>
                            {c.name}
                        </option>
                    ))}
                </select>

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Task
                </label>
                <select
                    value={taskId}
                    onChange={(e) => setTaskId(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">None</option>
                    {tasks.map((t) => (
                        <option key={t.id} value={t.id}>
                            {t.title}
                        </option>
                    ))}
                </select>

                <button
                    type="submit"
                    disabled={createNote.isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Create Note
                </button>
            </form>
        </Modal>
    );
}
