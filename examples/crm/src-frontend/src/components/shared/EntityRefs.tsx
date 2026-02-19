import { Link } from "react-router-dom";
import { useContacts, useCompanies, useDeals, useTasks } from "@/api/queries";
import { shortId } from "@/lib/format";

interface EntityRefsProps {
    dealId?: string | null;
    contactId?: string | null;
    companyId?: string | null;
    taskId?: string | null;
}

interface RefEntry {
    label: string;
    name: string;
    href: string;
}

export default function EntityRefs({ dealId, contactId, companyId, taskId }: EntityRefsProps) {
    const { data: contacts } = useContacts();
    const { data: companies } = useCompanies();
    const { data: deals } = useDeals();
    const { data: tasks } = useTasks();

    const refs: RefEntry[] = [];

    if (contactId) {
        const contact = contacts?.find((c) => c.id === contactId);
        refs.push({
            label: "Contact",
            name: contact?.name ?? shortId(contactId),
            href: `/contacts/${contactId}`,
        });
    }

    if (companyId) {
        const company = companies?.find((c) => c.id === companyId);
        refs.push({
            label: "Company",
            name: company?.name ?? shortId(companyId),
            href: `/companies/${companyId}`,
        });
    }

    if (dealId) {
        const deal = deals?.find((d) => d.id === dealId);
        refs.push({
            label: "Deal",
            name: deal?.title ?? shortId(dealId),
            href: `/deals/${dealId}`,
        });
    }

    if (taskId) {
        const task = tasks?.find((t) => t.id === taskId);
        refs.push({
            label: "Task",
            name: task?.title ?? shortId(taskId),
            href: `/tasks`,
        });
    }

    if (refs.length === 0) return null;

    return (
        <span className="text-sm text-gray-500 dark:text-gray-400">
            {refs.map((ref, i) => (
                <span key={ref.label}>
                    {i > 0 && " \u00b7 "}
                    {ref.label}:{" "}
                    <Link
                        to={ref.href}
                        className="font-semibold text-indigo-600 hover:underline"
                        onClick={(e) => e.stopPropagation()}
                    >
                        {ref.name}
                    </Link>
                </span>
            ))}
        </span>
    );
}
