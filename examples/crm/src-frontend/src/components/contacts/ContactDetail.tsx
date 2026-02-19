import { useState } from "react";
import { Link, useParams, useNavigate } from "react-router-dom";
import { useContact, useCompanies, useDeals, useTasks, useNotes } from "@/api/queries";
import { useAddContactTag, useLinkContactCompany, useArchiveContact } from "@/api/mutations";
import Badge from "@/components/shared/Badge";
import Card from "@/components/shared/Card";
import Currency from "@/components/shared/Currency";
import SectionHeader from "@/components/shared/SectionHeader";
import Modal from "@/components/shared/Modal";
import DealForm from "@/components/deals/DealForm";
import TaskForm from "@/components/tasks/TaskForm";
import TaskItem from "@/components/tasks/TaskItem";
import NoteForm from "@/components/notes/NoteForm";
import NoteCard from "@/components/notes/NoteCard";
import { formatStageName } from "@/lib/format";

/** Map DealStage to Badge variant. */
const STAGE_VARIANT: Record<string, string> = {
    Prospect: "prospect",
    Qualified: "qualified",
    Proposal: "proposal",
    Negotiation: "negotiation",
    ClosedWon: "won",
    ClosedLost: "lost",
};

export default function ContactDetail() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const { data: contact, isLoading } = useContact(id!);
    const { data: companies = [] } = useCompanies();
    const { data: deals = [] } = useDeals();
    const { data: tasks = [] } = useTasks();
    const { data: notes = [] } = useNotes();

    const addTag = useAddContactTag();
    const linkCompany = useLinkContactCompany();
    const archiveContact = useArchiveContact();

    const [showDealForm, setShowDealForm] = useState(false);
    const [showTaskForm, setShowTaskForm] = useState(false);
    const [showNoteForm, setShowNoteForm] = useState(false);
    const [showLinkCompany, setShowLinkCompany] = useState(false);
    const [selectedCompanyId, setSelectedCompanyId] = useState("");
    const [taskPrefill, setTaskPrefill] = useState<{
        dealId?: string | null;
        contactId?: string | null;
    }>({});

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    if (!contact) {
        return <p className="text-gray-500 dark:text-gray-400">Contact not found.</p>;
    }

    const company = companies.find((c) => c.id === contact.company_id);
    const linkedDeals = deals.filter((d) => d.contact_id === contact.id);
    const linkedTasks = tasks.filter((t) => t.contact_id === contact.id);
    const linkedNotes = notes.filter((n) => n.contact_id === contact.id);

    function handleAddTag() {
        const tag = window.prompt("Enter tag:");
        if (tag?.trim()) {
            addTag.mutate({ id: contact!.id, tag: tag.trim() });
        }
    }

    function handleArchive() {
        if (window.confirm("Archive this contact?")) {
            archiveContact.mutate({ id: contact!.id });
            void navigate("/contacts");
        }
    }

    function handleLinkCompanySubmit() {
        if (selectedCompanyId) {
            linkCompany.mutate({ id: contact!.id, companyId: selectedCompanyId });
        }
        setShowLinkCompany(false);
        setSelectedCompanyId("");
    }

    function handleNoteNewTask(dealId?: string | null, contactId?: string | null) {
        setTaskPrefill({ dealId: dealId ?? null, contactId: contactId ?? null });
        setShowTaskForm(true);
    }

    return (
        <div>
            <Link
                to="/contacts"
                className="mb-4 inline-block rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
            >
                Back to Contacts
            </Link>

            {/* Header */}
            <h2 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                {contact.name}
            </h2>
            {company && (
                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                    at{" "}
                    <Link
                        to={`/companies/${company.id}`}
                        className="font-semibold text-indigo-600 hover:underline"
                    >
                        {company.name}
                    </Link>
                </p>
            )}
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                {contact.email}
                {contact.phone && ` -- ${contact.phone}`}
            </p>
            {contact.tags.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-1">
                    {contact.tags.map((tag) => (
                        <Badge key={tag} variant="tag">
                            {tag}
                        </Badge>
                    ))}
                </div>
            )}

            {/* Actions */}
            <div className="mt-3 flex gap-2">
                <button
                    type="button"
                    onClick={handleAddTag}
                    className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                >
                    + Tag
                </button>
                <button
                    type="button"
                    onClick={() => setShowLinkCompany(true)}
                    className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                >
                    Link Company
                </button>
                <button
                    type="button"
                    onClick={handleArchive}
                    className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                >
                    Archive
                </button>
            </div>

            {/* Deals section */}
            <div className="mt-5">
                <SectionHeader
                    title="Deals"
                    count={linkedDeals.length}
                    onAdd={() => setShowDealForm(true)}
                />
                {linkedDeals.length > 0 && (
                    <div className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
                        {linkedDeals.map((deal) => (
                            <Card
                                key={deal.id}
                                clickable
                                onClick={() => navigate(`/deals/${deal.id}`)}
                            >
                                <p className="font-semibold text-gray-900 dark:text-gray-100">
                                    {deal.title}
                                </p>
                                <div className="mt-1 flex items-center gap-2">
                                    <Badge variant={STAGE_VARIANT[deal.stage] ?? "system"}>
                                        {formatStageName(deal.stage)}
                                    </Badge>
                                    <Currency
                                        cents={deal.value}
                                        className="text-sm font-semibold text-indigo-600"
                                    />
                                </div>
                            </Card>
                        ))}
                    </div>
                )}
            </div>

            {/* Tasks section */}
            <div className="mt-5">
                <SectionHeader
                    title="Tasks"
                    count={linkedTasks.length}
                    onAdd={() => {
                        setTaskPrefill({ contactId: contact.id });
                        setShowTaskForm(true);
                    }}
                />
                <div className="mt-3">
                    {linkedTasks.map((task) => (
                        <TaskItem key={task.id} task={task} />
                    ))}
                </div>
            </div>

            {/* Notes section */}
            <div className="mt-5">
                <SectionHeader
                    title="Notes"
                    count={linkedNotes.length}
                    onAdd={() => setShowNoteForm(true)}
                />
                <div className="mt-3">
                    {linkedNotes.map((note) => (
                        <NoteCard
                            key={note.id}
                            note={note}
                            onNewTask={(dealId, contactId) => handleNoteNewTask(dealId, contactId)}
                        />
                    ))}
                </div>
            </div>

            {/* Modals */}
            {showDealForm && (
                <DealForm
                    onClose={() => setShowDealForm(false)}
                    prefillContactId={contact.id}
                    prefillCompanyId={contact.company_id}
                />
            )}
            {showTaskForm && (
                <TaskForm
                    onClose={() => {
                        setShowTaskForm(false);
                        setTaskPrefill({});
                    }}
                    prefillContactId={taskPrefill.contactId ?? contact.id}
                    prefillDealId={taskPrefill.dealId ?? null}
                />
            )}
            {showNoteForm && (
                <NoteForm onClose={() => setShowNoteForm(false)} prefillContactId={contact.id} />
            )}
            <Modal
                open={showLinkCompany}
                title="Link Company"
                onClose={() => {
                    setShowLinkCompany(false);
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
