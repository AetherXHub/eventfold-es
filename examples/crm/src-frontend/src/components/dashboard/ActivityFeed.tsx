import { useMemo } from "react";
import { Link } from "react-router-dom";
import {
    useActivityFeed,
    useContacts,
    useCompanies,
    useDeals,
    useTasks,
    useNotes,
} from "@/api/queries";
import Badge from "@/components/shared/Badge";
import { AGGREGATE_ROUTE } from "@/lib/constants";
import { formatEventType, shortId } from "@/lib/format";

export default function ActivityFeed() {
    const { data: activity, isLoading } = useActivityFeed();
    const { data: contacts } = useContacts();
    const { data: companies } = useCompanies();
    const { data: deals } = useDeals();
    const { data: tasks } = useTasks();
    const { data: notes } = useNotes();

    // Build name lookup maps keyed by entity id.
    const nameMap = useMemo(() => {
        const map = new Map<string, string>();
        contacts?.forEach((c) => map.set(c.id, c.name));
        companies?.forEach((c) => map.set(c.id, c.name));
        deals?.forEach((d) => map.set(d.id, d.title));
        tasks?.forEach((t) => map.set(t.id, t.title));
        notes?.forEach((n) => map.set(n.id, n.body.slice(0, 40)));
        return map;
    }, [contacts, companies, deals, tasks, notes]);

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    const entries = activity?.entries.slice(0, 50) ?? [];

    if (entries.length === 0) {
        return <p className="text-sm text-gray-500 dark:text-gray-400">No activity yet.</p>;
    }

    return (
        <div>
            {entries.map((entry, i) => {
                const verb = formatEventType(entry.event_type);
                const entityName = nameMap.get(entry.instance_id);
                const route = AGGREGATE_ROUTE[entry.aggregate_type] ?? "/";
                const variantMap: Record<string, string> = {
                    contact: "prospect",
                    company: "qualified",
                    deal: "proposal",
                    task: "negotiation",
                    note: "system",
                };
                const variant = variantMap[entry.aggregate_type] ?? "system";

                return (
                    <div
                        key={`${entry.instance_id}-${entry.ts}-${i}`}
                        className="flex items-center gap-3 border-b border-gray-100 py-2 text-sm text-gray-600 last:border-b-0 dark:border-gray-800 dark:text-gray-400"
                    >
                        <Badge variant={variant}>{entry.aggregate_type}</Badge>
                        <span className="flex-1">
                            {verb} {entry.aggregate_type}{" "}
                            <Link
                                to={`${route}/${entry.instance_id}`}
                                className="text-indigo-600 hover:underline"
                            >
                                {entityName ? (
                                    <strong>{entityName}</strong>
                                ) : (
                                    <code className="rounded bg-gray-100 px-1 text-xs dark:bg-gray-800">
                                        {shortId(entry.instance_id)}
                                    </code>
                                )}
                            </Link>
                        </span>
                        <span className="text-xs whitespace-nowrap text-gray-400 dark:text-gray-500">
                            {new Date(entry.ts * 1000).toLocaleString()}
                        </span>
                    </div>
                );
            })}
        </div>
    );
}
