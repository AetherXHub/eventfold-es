import { useState } from "react";
import { useDeals } from "@/api/queries";
import type { DealStage } from "@/api/types";
import Badge from "@/components/shared/Badge";
import DealCard from "@/components/deals/DealCard";
import DealForm from "@/components/deals/DealForm";
import { DEAL_STAGES } from "@/lib/constants";
import { formatStageName } from "@/lib/format";

/** Maps DealStage to the Badge variant string. */
const STAGE_VARIANT: Record<DealStage, string> = {
    Prospect: "prospect",
    Qualified: "qualified",
    Proposal: "proposal",
    Negotiation: "negotiation",
    ClosedWon: "won",
    ClosedLost: "lost",
};

export default function DealBoard() {
    const { data: deals, isLoading } = useDeals();
    const [showForm, setShowForm] = useState(false);

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    const allDeals = deals ?? [];

    return (
        <div>
            <div className="mb-4 flex items-center justify-between">
                <h1 className="font-heading text-2xl font-semibold text-gray-900 dark:text-gray-100">
                    Deals
                </h1>
                <button
                    type="button"
                    onClick={() => setShowForm(true)}
                    className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-indigo-700"
                >
                    New Deal
                </button>
            </div>

            {showForm && <DealForm onClose={() => setShowForm(false)} />}

            <div className="kanban-scroll flex gap-3 overflow-x-auto pb-2">
                {DEAL_STAGES.map((stage) => {
                    const stageDeals = allDeals.filter((d) => d.stage === stage);
                    return (
                        <div
                            key={stage}
                            className="min-w-0 flex-1 rounded-lg border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-800/50"
                        >
                            <div className="mb-2 flex items-center gap-1.5 border-b-2 border-gray-200 pb-2 text-xs font-semibold tracking-wider whitespace-nowrap text-gray-500 uppercase dark:border-gray-700 dark:text-gray-400">
                                <Badge variant={STAGE_VARIANT[stage]}>
                                    {formatStageName(stage)}
                                </Badge>
                                <span>{stageDeals.length}</span>
                            </div>
                            {stageDeals.length === 0 ? (
                                <p className="text-sm text-gray-500 dark:text-gray-400">No deals</p>
                            ) : (
                                stageDeals.map((deal) => <DealCard key={deal.id} deal={deal} />)
                            )}
                        </div>
                    );
                })}
            </div>
        </div>
    );
}
