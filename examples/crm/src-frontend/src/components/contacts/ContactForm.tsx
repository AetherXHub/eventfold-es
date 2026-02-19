import { useState } from "react";
import Modal from "@/components/shared/Modal";
import { useCreateContact } from "@/api/mutations";

interface ContactFormProps {
    onClose: () => void;
}

export default function ContactForm({ onClose }: ContactFormProps) {
    const [name, setName] = useState("");
    const [email, setEmail] = useState("");
    const [phone, setPhone] = useState("");

    const createContact = useCreateContact();

    async function handleSubmit(e: React.SyntheticEvent) {
        e.preventDefault();
        await createContact.mutateAsync({
            id: crypto.randomUUID(),
            name,
            email,
            phone,
        });
        onClose();
    }

    return (
        <Modal open title="New Contact" onClose={onClose}>
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
                    Email
                </label>
                <input
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Phone
                </label>
                <input
                    type="tel"
                    value={phone}
                    onChange={(e) => setPhone(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                />

                <button
                    type="submit"
                    disabled={createContact.isPending}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Create Contact
                </button>
            </form>
        </Modal>
    );
}
