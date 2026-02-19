import { useState } from "react";
import { useNotes } from "@/api/queries";
import EmptyState from "@/components/shared/EmptyState";
import NoteForm from "@/components/notes/NoteForm";
import NoteCard from "@/components/notes/NoteCard";
import TaskForm from "@/components/tasks/TaskForm";

export default function NoteList() {
    const { data: notes, isLoading } = useNotes();
    const [showForm, setShowForm] = useState(false);
    const [showTaskForm, setShowTaskForm] = useState(false);
    const [taskPrefill, setTaskPrefill] = useState<{
        dealId?: string | null;
        contactId?: string | null;
    }>({});

    function handleNewTask(dealId?: string | null, contactId?: string | null) {
        setTaskPrefill({ dealId: dealId ?? null, contactId: contactId ?? null });
        setShowTaskForm(true);
    }

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    const allNotes = notes ?? [];

    return (
        <div>
            <div className="mb-4 flex items-center justify-between">
                <h1 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                    Notes
                </h1>
                <button
                    type="button"
                    onClick={() => setShowForm(true)}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
                >
                    New Note
                </button>
            </div>

            {showForm && <NoteForm onClose={() => setShowForm(false)} />}

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

            {allNotes.length === 0 ? (
                <EmptyState message="No notes yet. Create your first note." />
            ) : (
                <div>
                    {allNotes.map((note) => (
                        <NoteCard
                            key={note.id}
                            note={note}
                            onNewTask={(dealId, contactId) => handleNewTask(dealId, contactId)}
                        />
                    ))}
                </div>
            )}
        </div>
    );
}
