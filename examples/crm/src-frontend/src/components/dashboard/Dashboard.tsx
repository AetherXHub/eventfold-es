import PipelineBoard from "@/components/dashboard/PipelineBoard";
import ActivityFeed from "@/components/dashboard/ActivityFeed";

export default function Dashboard() {
    return (
        <div>
            <h1 className="font-heading mb-4 text-2xl font-semibold text-gray-900 dark:text-gray-100">
                Dashboard
            </h1>
            <PipelineBoard />
            <h2 className="font-heading mt-5 mb-3 text-lg font-semibold text-gray-600 dark:text-gray-400">
                Recent Activity
            </h2>
            <ActivityFeed />
        </div>
    );
}
