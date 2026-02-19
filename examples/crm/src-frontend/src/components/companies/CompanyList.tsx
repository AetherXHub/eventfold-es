import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useCompanies } from "@/api/queries";
import { useArchiveCompany } from "@/api/mutations";
import Card from "@/components/shared/Card";
import EmptyState from "@/components/shared/EmptyState";
import CompanyForm from "@/components/companies/CompanyForm";

export default function CompanyList() {
    const { data: companies, isLoading } = useCompanies();
    const archiveCompany = useArchiveCompany();
    const navigate = useNavigate();

    const [showForm, setShowForm] = useState(false);

    function handleArchive(id: string) {
        if (window.confirm("Archive this company?")) {
            archiveCompany.mutate({ id });
        }
    }

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    return (
        <div>
            <div className="mb-4 flex items-center justify-between">
                <h1 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                    Companies
                </h1>
                <button
                    type="button"
                    onClick={() => setShowForm(true)}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
                >
                    New Company
                </button>
            </div>

            {showForm && <CompanyForm onClose={() => setShowForm(false)} />}

            {!companies || companies.length === 0 ? (
                <EmptyState message="No companies yet. Create your first company." />
            ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3">
                    {companies.map((c) => (
                        <Card key={c.id} clickable onClick={() => navigate(`/companies/${c.id}`)}>
                            <p className="font-semibold text-gray-900 dark:text-gray-100">
                                {c.name}
                            </p>
                            <p className="text-sm text-gray-500 dark:text-gray-400">{c.industry}</p>
                            {c.website && (
                                <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
                                    {c.website}
                                </p>
                            )}
                            <div className="mt-3 flex gap-2">
                                <button
                                    type="button"
                                    onClick={(e) => {
                                        e.stopPropagation();
                                        handleArchive(c.id);
                                    }}
                                    className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                                >
                                    Archive
                                </button>
                            </div>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}
