import type { TaskWithId } from "@/api/types";
import { useCompleteTask, useReopenTask } from "@/api/mutations";
import Badge from "@/components/shared/Badge";
import EntityRefs from "@/components/shared/EntityRefs";

interface TaskItemProps {
    task: TaskWithId;
}

export default function TaskItem({ task }: TaskItemProps) {
    const completeTask = useCompleteTask();
    const reopenTask = useReopenTask();

    function handleToggle() {
        if (task.done) {
            reopenTask.mutate({ id: task.id });
        } else {
            completeTask.mutate({ id: task.id });
        }
    }

    const isSystemTask = task.id.startsWith("auto-task-");

    return (
        <div className="flex items-center gap-3 border-b border-gray-100 py-2.5 last:border-b-0 dark:border-gray-800">
            <input
                type="checkbox"
                checked={task.done}
                onChange={handleToggle}
                className="shrink-0"
            />

            <div className="min-w-0 flex-1">
                <span
                    className={`font-body text-sm text-gray-900 dark:text-gray-100 ${
                        task.done ? "line-through opacity-50" : ""
                    }`}
                >
                    {task.title}
                </span>

                {isSystemTask && (
                    <span className="ml-2">
                        <Badge variant="system">system</Badge>
                    </span>
                )}

                <div className="mt-0.5">
                    <EntityRefs dealId={task.deal_id} contactId={task.contact_id} />
                </div>
            </div>

            {task.due && (
                <span className="shrink-0 text-xs text-gray-400 dark:text-gray-500">
                    {task.due}
                </span>
            )}
        </div>
    );
}
