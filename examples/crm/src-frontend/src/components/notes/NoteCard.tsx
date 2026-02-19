import type { NoteWithId } from "@/api/types";
import Card from "@/components/shared/Card";
import EntityRefs from "@/components/shared/EntityRefs";

interface NoteCardProps {
    note: NoteWithId;
    onNewTask?: (dealId: string | null, contactId: string | null) => void;
}

export default function NoteCard({ note, onNewTask }: NoteCardProps) {
    return (
        <Card>
            <p className="font-body text-sm whitespace-pre-wrap text-gray-900 dark:text-gray-100">
                {note.body}
            </p>

            <div className="mt-2 flex items-center justify-between gap-2">
                <EntityRefs
                    contactId={note.contact_id}
                    companyId={note.company_id}
                    dealId={note.deal_id}
                    taskId={note.task_id}
                />

                {onNewTask && (
                    <button
                        type="button"
                        onClick={(e) => {
                            e.stopPropagation();
                            onNewTask(note.deal_id, note.contact_id);
                        }}
                        className="shrink-0 rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                    >
                        + Task
                    </button>
                )}
            </div>
        </Card>
    );
}
