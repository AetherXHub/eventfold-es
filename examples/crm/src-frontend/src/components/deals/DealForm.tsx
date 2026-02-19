import { useState, useEffect, useMemo } from "react";
import Modal from "@/components/shared/Modal";
import { useCreateDeal } from "@/api/mutations";
import { useCompanies, useContacts } from "@/api/queries";

interface DealFormProps {
    onClose: () => void;
    prefillCompanyId?: string | null;
    prefillContactId?: string | null;
}

export default function DealForm({ onClose, prefillCompanyId, prefillContactId }: DealFormProps) {
    const { data: companies = [] } = useCompanies();
    const { data: contacts = [] } = useContacts();
    const createDeal = useCreateDeal();

    const [companyId, setCompanyId] = useState(prefillCompanyId ?? "");
    const [contactId, setContactId] = useState(prefillContactId ?? "");
    const [title, setTitle] = useState("");
    const [value, setValue] = useState("");

    // On mount: resolve prefills. If prefillContactId has a company, auto-select it.
    useEffect(() => {
        if (prefillContactId) {
            const contact = contacts.find((c) => c.id === prefillContactId);
            if (contact?.company_id) {
                setCompanyId(contact.company_id);
            }
        }
    }, [prefillContactId, contacts]);

    // Filter contacts: show those matching selected company or with no company.
    const filteredContacts = useMemo(() => {
        if (!companyId) return contacts;
        return contacts.filter((c) => c.company_id === companyId || c.company_id === null);
    }, [contacts, companyId]);

    // When company changes, preserve contact selection only if still valid.
    useEffect(() => {
        if (contactId && !filteredContacts.some((c) => c.id === contactId)) {
            setContactId("");
        }
    }, [filteredContacts, contactId]);

    function handleCompanyChange(newCompanyId: string) {
        setCompanyId(newCompanyId);
    }

    function handleContactChange(newContactId: string) {
        setContactId(newContactId);
        // If the selected contact has a company, auto-select that company.
        const contact = contacts.find((c) => c.id === newContactId);
        if (contact?.company_id) {
            setCompanyId(contact.company_id);
        }
    }

    async function handleSubmit(e: React.SyntheticEvent) {
        e.preventDefault();
        const cents = Math.round(parseFloat(value) * 100) || 0;
        await createDeal.mutateAsync({
            id: crypto.randomUUID(),
            title,
            companyId,
            contactId,
            value: cents,
        });
        onClose();
    }

    return (
        <Modal open title="New Deal" onClose={onClose}>
            <form onSubmit={handleSubmit}>
                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Company
                </label>
                <select
                    value={companyId}
                    onChange={(e) => handleCompanyChange(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">Select company...</option>
                    {companies.map((c) => (
                        <option key={c.id} value={c.id}>
                            {c.name}
                        </option>
                    ))}
                </select>

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Contact
                </label>
                <select
                    value={contactId}
                    onChange={(e) => handleContactChange(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">Select contact...</option>
                    {filteredContacts.map((c) => (
                        <option key={c.id} value={c.id}>
                            {c.name}
                        </option>
                    ))}
                </select>

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
                    Value ($)
                </label>
                <input
                    type="number"
                    step="0.01"
                    placeholder="0.00"
                    value={value}
                    onChange={(e) => setValue(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <button
                    type="submit"
                    disabled={createDeal.isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Create Deal
                </button>
            </form>
        </Modal>
    );
}
