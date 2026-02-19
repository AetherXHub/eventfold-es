import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useContacts, useCompanies } from "@/api/queries";
import { useAddContactTag, useLinkContactCompany, useArchiveContact } from "@/api/mutations";
import Card from "@/components/shared/Card";
import Badge from "@/components/shared/Badge";
import EmptyState from "@/components/shared/EmptyState";
import Modal from "@/components/shared/Modal";
import ContactForm from "@/components/contacts/ContactForm";

export default function ContactList() {
    const { data: contacts, isLoading } = useContacts();
    const { data: companies = [] } = useCompanies();
    const navigate = useNavigate();

    const addTag = useAddContactTag();
    const linkCompany = useLinkContactCompany();
    const archiveContact = useArchiveContact();

    const [showForm, setShowForm] = useState(false);
    const [linkCompanyTarget, setLinkCompanyTarget] = useState<string | null>(null);
    const [selectedCompanyId, setSelectedCompanyId] = useState("");

    function companyName(companyId: string | null): string | null {
        if (!companyId) return null;
        return companies.find((c) => c.id === companyId)?.name ?? null;
    }

    function handleAddTag(id: string) {
        const tag = window.prompt("Enter tag:");
        if (tag?.trim()) {
            addTag.mutate({ id, tag: tag.trim() });
        }
    }

    function handleArchive(id: string) {
        if (window.confirm("Archive this contact?")) {
            archiveContact.mutate({ id });
        }
    }

    function handleLinkCompanySubmit() {
        if (linkCompanyTarget && selectedCompanyId) {
            linkCompany.mutate({ id: linkCompanyTarget, companyId: selectedCompanyId });
        }
        setLinkCompanyTarget(null);
        setSelectedCompanyId("");
    }

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    return (
        <div>
            <div className="mb-4 flex items-center justify-between">
                <h1 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                    Contacts
                </h1>
                <button
                    type="button"
                    onClick={() => setShowForm(true)}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
                >
                    New Contact
                </button>
            </div>

            {showForm && <ContactForm onClose={() => setShowForm(false)} />}

            {!contacts || contacts.length === 0 ? (
                <EmptyState message="No contacts yet. Create your first contact." />
            ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
                    {contacts.map((c) => {
                        const company = companyName(c.company_id);
                        return (
                            <Card
                                key={c.id}
                                clickable
                                onClick={() => navigate(`/contacts/${c.id}`)}
                            >
                                <p className="font-semibold text-gray-900 dark:text-gray-100">
                                    {c.name}
                                </p>
                                {company && (
                                    <p className="text-sm text-gray-500 dark:text-gray-400">
                                        at <strong>{company}</strong>
                                    </p>
                                )}
                                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                                    {c.email}
                                    {c.phone && ` -- ${c.phone}`}
                                </p>
                                {c.tags.length > 0 && (
                                    <div className="mt-2 flex flex-wrap gap-1">
                                        {c.tags.map((tag) => (
                                            <Badge key={tag} variant="tag">
                                                {tag}
                                            </Badge>
                                        ))}
                                    </div>
                                )}
                                <div className="mt-3 flex gap-2">
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleAddTag(c.id);
                                        }}
                                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                                    >
                                        + Tag
                                    </button>
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            setLinkCompanyTarget(c.id);
                                        }}
                                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                                    >
                                        Link Company
                                    </button>
                                    <button
                                        type="button"
                                        onClick={(e) => {
                                            e.stopPropagation();
                                            handleArchive(c.id);
                                        }}
                                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                                    >
                                        Archive
                                    </button>
                                </div>
                            </Card>
                        );
                    })}
                </div>
            )}

            {/* Link Company Modal */}
            <Modal
                open={linkCompanyTarget !== null}
                title="Link Company"
                onClose={() => {
                    setLinkCompanyTarget(null);
                    setSelectedCompanyId("");
                }}
            >
                <label className="mb-1 block text-sm font-medium text-gray-600 dark:text-gray-400">
                    Company
                </label>
                <select
                    value={selectedCompanyId}
                    onChange={(e) => setSelectedCompanyId(e.target.value)}
                    className="mb-3 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 focus:border-indigo-600 focus:outline-2 focus:outline-indigo-600/20 dark:border-gray-700 dark:bg-gray-800 dark:text-gray-100"
                >
                    <option value="">Select company...</option>
                    {companies.map((co) => (
                        <option key={co.id} value={co.id}>
                            {co.name}
                        </option>
                    ))}
                </select>
                <button
                    type="button"
                    onClick={handleLinkCompanySubmit}
                    disabled={!selectedCompanyId}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700 disabled:opacity-50"
                >
                    Link
                </button>
            </Modal>
        </div>
    );
}
