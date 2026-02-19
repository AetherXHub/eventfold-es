import { useState } from "react";
import Modal from "@/components/shared/Modal";
import { useCreateTask } from "@/api/mutations";
import { useDeals, useContacts } from "@/api/queries";

interface TaskFormProps {
    onClose: () => void;
    prefillDealId?: string | null;
    prefillContactId?: string | null;
}

export default function TaskForm({ onClose, prefillDealId, prefillContactId }: TaskFormProps) {
    const { data: deals = [] } = useDeals();
    const { data: contacts = [] } = useContacts();
    const createTask = useCreateTask();

    const [title, setTitle] = useState("");
    const [description, setDescription] = useState("");
    const [dealId, setDealId] = useState(prefillDealId ?? "");
    const [contactId, setContactId] = useState(prefillContactId ?? "");
    const [due, setDue] = useState("");

    async function handleSubmit(e: React.SyntheticEvent) {
        e.preventDefault();
        await createTask.mutateAsync({
            id: crypto.randomUUID(),
            title,
            description,
            dealId: dealId || null,
            contactId: contactId || null,
            due: due || null,
        });
        onClose();
    }

    return (
        <Modal open title="New Task" onClose={onClose}>
            <form onSubmit={handleSubmit}>
                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Title
                </label>
                <input
                    type="text"
                    required
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Description
                </label>
                <textarea
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    rows={3}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

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
                    Due date
                </label>
                <input
                    type="date"
                    value={due}
                    onChange={(e) => setDue(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <button
                    type="submit"
                    disabled={createTask.isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Create Task
                </button>
            </form>
        </Modal>
    );
}
