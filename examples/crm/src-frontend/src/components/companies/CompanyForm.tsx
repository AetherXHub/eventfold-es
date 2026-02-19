import { useState } from "react";
import Modal from "@/components/shared/Modal";
import { useCreateCompany } from "@/api/mutations";

interface CompanyFormProps {
    onClose: () => void;
}

export default function CompanyForm({ onClose }: CompanyFormProps) {
    const [name, setName] = useState("");
    const [industry, setIndustry] = useState("");
    const [website, setWebsite] = useState("");

    const createCompany = useCreateCompany();

    async function handleSubmit(e: React.SyntheticEvent) {
        e.preventDefault();
        await createCompany.mutateAsync({
            id: crypto.randomUUID(),
            name,
            industry,
            website: website || null,
        });
        onClose();
    }

    return (
        <Modal open title="New Company" onClose={onClose}>
            <form onSubmit={handleSubmit}>
                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Name
                </label>
                <input
                    type="text"
                    required
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Industry
                </label>
                <input
                    type="text"
                    value={industry}
                    onChange={(e) => setIndustry(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Website
                </label>
                <input
                    type="url"
                    value={website}
                    onChange={(e) => setWebsite(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <button
                    type="submit"
                    disabled={createCompany.isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Create Company
                </button>
            </form>
        </Modal>
    );
}
