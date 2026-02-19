import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useDeal, useContacts, useCompanies, useTasks, useNotes } from "@/api/queries";
import { useAdvanceDeal, useWinDeal, useLoseDeal } from "@/api/mutations";
import Badge from "@/components/shared/Badge";
import Currency from "@/components/shared/Currency";
import SectionHeader from "@/components/shared/SectionHeader";
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

export default function DealDetail() {
    const { id } = useParams<{ id: string }>();
    const { data: deal, isLoading } = useDeal(id!);
    const { data: contacts = [] } = useContacts();
    const { data: companies = [] } = useCompanies();
    const { data: tasks = [] } = useTasks();
    const { data: notes = [] } = useNotes();

    const advanceDeal = useAdvanceDeal();
    const winDeal = useWinDeal();
    const loseDeal = useLoseDeal();

    const [showTaskForm, setShowTaskForm] = useState(false);
    const [showNoteForm, setShowNoteForm] = useState(false);
    const [taskPrefill, setTaskPrefill] = useState<{
        dealId?: string | null;
        contactId?: string | null;
    }>({});

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    if (!deal) {
        return <p className="text-gray-500 dark:text-gray-400">Deal not found.</p>;
    }

    const company = companies.find((c) => c.id === deal.company_id);
    const contact = contacts.find((c) => c.id === deal.contact_id);
    const linkedTasks = tasks.filter((t) => t.deal_id === deal.id);
    const linkedNotes = notes.filter((n) => n.deal_id === deal.id);

    const showAdvance =
        !deal.closed &&
        deal.stage !== "Negotiation" &&
        deal.stage !== "ClosedWon" &&
        deal.stage !== "ClosedLost";
    const showWinLose = !deal.closed;

    function handleWin() {
        if (window.confirm("Mark this deal as won?")) {
            winDeal.mutate({ id: deal!.id });
        }
    }

    function handleLose() {
        if (window.confirm("Mark this deal as lost?")) {
            loseDeal.mutate({ id: deal!.id });
        }
    }

    function handleNoteNewTask(dealId?: string | null, contactId?: string | null) {
        setTaskPrefill({ dealId: dealId ?? null, contactId: contactId ?? null });
        setShowTaskForm(true);
    }

    return (
        <div>
            <Link
                to="/deals"
                className="mb-4 inline-block rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
            >
                Back to Deals
            </Link>

            {/* Header */}
            <h2 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                {deal.title}
            </h2>
            <div className="mt-2 flex items-center gap-3">
                <Badge variant={STAGE_VARIANT[deal.stage] ?? "system"}>
                    {formatStageName(deal.stage)}
                </Badge>
                <Currency
                    cents={deal.value}
                    className="font-heading text-lg font-semibold text-indigo-600"
                />
            </div>
            <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
                {company && (
                    <span>
                        Company:{" "}
                        <Link
                            to={`/companies/${company.id}`}
                            className="font-semibold text-indigo-600 hover:underline"
                        >
                            {company.name}
                        </Link>
                    </span>
                )}
                {company && contact && " \u00b7 "}
                {contact && (
                    <span>
                        Contact:{" "}
                        <Link
                            to={`/contacts/${contact.id}`}
                            className="font-semibold text-indigo-600 hover:underline"
                        >
                            {contact.name}
                        </Link>
                    </span>
                )}
            </p>

            {/* Actions */}
            {showWinLose && (
                <div className="mt-3 flex gap-2">
                    {showAdvance && (
                        <button
                            type="button"
                            onClick={() => advanceDeal.mutate({ id: deal.id })}
                            className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                        >
                            Advance
                        </button>
                    )}
                    <button
                        type="button"
                        onClick={handleWin}
                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-green-500 transition-colors hover:border-green-500 hover:bg-green-50 dark:border-gray-700 dark:hover:bg-green-950/20"
                    >
                        Win
                    </button>
                    <button
                        type="button"
                        onClick={handleLose}
                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                    >
                        Lose
                    </button>
                </div>
            )}

            {/* Tasks section */}
            <div className="mt-5">
                <SectionHeader
                    title="Tasks"
                    count={linkedTasks.length}
                    onAdd={() => {
                        setTaskPrefill({ dealId: deal.id });
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
            {showTaskForm && (
                <TaskForm
                    onClose={() => {
                        setShowTaskForm(false);
                        setTaskPrefill({});
                    }}
                    prefillDealId={taskPrefill.dealId ?? deal.id}
                    prefillContactId={taskPrefill.contactId ?? null}
                />
            )}
            {showNoteForm && (
                <NoteForm onClose={() => setShowNoteForm(false)} prefillDealId={deal.id} />
            )}
        </div>
    );
}
