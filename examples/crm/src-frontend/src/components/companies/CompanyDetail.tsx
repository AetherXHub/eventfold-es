import { useState } from "react";
import { Link, useParams, useNavigate } from "react-router-dom";
import { useCompany, useContacts, useDeals, useNotes } from "@/api/queries";
import { useArchiveCompany } from "@/api/mutations";
import Badge from "@/components/shared/Badge";
import Card from "@/components/shared/Card";
import Currency from "@/components/shared/Currency";
import SectionHeader from "@/components/shared/SectionHeader";
import DealForm from "@/components/deals/DealForm";
import TaskForm from "@/components/tasks/TaskForm";
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

export default function CompanyDetail() {
    const { id } = useParams<{ id: string }>();
    const navigate = useNavigate();
    const { data: company, isLoading } = useCompany(id!);
    const { data: contacts = [] } = useContacts();
    const { data: deals = [] } = useDeals();
    const { data: notes = [] } = useNotes();

    const archiveCompany = useArchiveCompany();

    const [showDealForm, setShowDealForm] = useState(false);
    const [showNoteForm, setShowNoteForm] = useState(false);
    const [showTaskForm, setShowTaskForm] = useState(false);
    const [taskPrefill, setTaskPrefill] = useState<{
        dealId?: string | null;
        contactId?: string | null;
    }>({});

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    if (!company) {
        return <p className="text-gray-500 dark:text-gray-400">Company not found.</p>;
    }

    const linkedContacts = contacts.filter((c) => c.company_id === company.id);
    const linkedDeals = deals.filter((d) => d.company_id === company.id);
    const linkedNotes = notes.filter((n) => n.company_id === company.id);

    function handleArchive() {
        if (window.confirm("Archive this company?")) {
            archiveCompany.mutate({ id: company!.id });
            void navigate("/companies");
        }
    }

    function handleNoteNewTask(dealId?: string | null, contactId?: string | null) {
        setTaskPrefill({ dealId: dealId ?? null, contactId: contactId ?? null });
        setShowTaskForm(true);
    }

    return (
        <div>
            <Link
                to="/companies"
                className="mb-4 inline-block rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
            >
                Back to Companies
            </Link>

            {/* Header */}
            <h2 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                {company.name}
            </h2>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">{company.industry}</p>
            {company.website && (
                <a
                    href={company.website}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="mt-1 inline-block text-sm text-indigo-600 hover:underline"
                >
                    {company.website}
                </a>
            )}

            {/* Actions */}
            <div className="mt-3 flex gap-2">
                <button
                    type="button"
                    onClick={handleArchive}
                    className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                >
                    Archive
                </button>
            </div>

            {/* Contacts section */}
            <div className="mt-5">
                <SectionHeader title="Contacts" count={linkedContacts.length} />
                {linkedContacts.length > 0 && (
                    <div className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
                        {linkedContacts.map((c) => (
                            <Card
                                key={c.id}
                                clickable
                                onClick={() => navigate(`/contacts/${c.id}`)}
                            >
                                <p className="font-semibold text-gray-900 dark:text-gray-100">
                                    {c.name}
                                </p>
                                <p className="text-sm text-gray-500 dark:text-gray-400">
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
                            </Card>
                        ))}
                    </div>
                )}
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
                <DealForm onClose={() => setShowDealForm(false)} prefillCompanyId={company.id} />
            )}
            {showNoteForm && (
                <NoteForm onClose={() => setShowNoteForm(false)} prefillCompanyId={company.id} />
            )}
            {showTaskForm && (
                <TaskForm
                    onClose={() => {
                        setShowTaskForm(false);
                        setTaskPrefill({});
                    }}
                    prefillDealId={taskPrefill.dealId ?? null}
                    prefillContactId={taskPrefill.contactId ?? null}
                />
            )}
        </div>
    );
}
