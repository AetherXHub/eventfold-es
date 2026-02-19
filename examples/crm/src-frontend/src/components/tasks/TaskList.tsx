import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useTasks } from "@/api/queries";
import EmptyState from "@/components/shared/EmptyState";
import TaskForm from "@/components/tasks/TaskForm";
import TaskItem from "@/components/tasks/TaskItem";

export default function TaskList() {
    const { data: tasks, isLoading } = useTasks();
    const [searchParams, setSearchParams] = useSearchParams();
    const [showForm, setShowForm] = useState(false);

    const filter = searchParams.get("filter") ?? "pending";

    function setFilter(value: string) {
        setSearchParams({ filter: value });
    }

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    const allTasks = tasks ?? [];
    const filtered =
        filter === "completed" ? allTasks.filter((t) => t.done) : allTasks.filter((t) => !t.done);

    const tabClass = (tab: string) =>
        tab === filter
            ? "border-b-2 border-indigo-600 text-indigo-600 font-semibold px-3 py-2 text-sm"
            : "border-b-2 border-transparent text-gray-400 hover:text-gray-600 px-3 py-2 text-sm";

    return (
        <div>
            <div className="mb-4 flex items-center justify-between">
                <h1 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                    Tasks
                </h1>
                <button
                    type="button"
                    onClick={() => setShowForm(true)}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
                >
                    New Task
                </button>
            </div>

            {showForm && <TaskForm onClose={() => setShowForm(false)} />}

            {/* Tabs */}
            <div className="mb-4 flex gap-2 border-b border-gray-200 dark:border-gray-700">
                <button
                    type="button"
                    onClick={() => setFilter("pending")}
                    className={tabClass("pending")}
                >
                    Pending
                </button>
                <button
                    type="button"
                    onClick={() => setFilter("completed")}
                    className={tabClass("completed")}
                >
                    Completed
                </button>
            </div>

            {filtered.length === 0 ? (
                <EmptyState
                    message={filter === "completed" ? "No completed tasks." : "No pending tasks."}
                />
            ) : (
                <div>
                    {filtered.map((task) => (
                        <TaskItem key={task.id} task={task} />
                    ))}
                </div>
            )}
        </div>
    );
}
