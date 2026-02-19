import { Link } from "react-router-dom";
import { usePipelineSummary } from "@/api/queries";
import type { DealStage } from "@/api/types";
import Badge from "@/components/shared/Badge";
import { DEAL_STAGES } from "@/lib/constants";
import { formatStageName, formatCurrency } from "@/lib/format";

/** Maps DealStage to the Badge variant string. */
const STAGE_VARIANT: Record<DealStage, string> = {
    Prospect: "prospect",
    Qualified: "qualified",
    Proposal: "proposal",
    Negotiation: "negotiation",
    ClosedWon: "won",
    ClosedLost: "lost",
};

export default function PipelineBoard() {
    const { data: pipeline, isLoading } = usePipelineSummary();

    if (isLoading) {
        return <p className="text-gray-500 dark:text-gray-400">Loading...</p>;
    }

    if (!pipeline) return null;

    return (
        <div>
            {/* Kanban board */}
            <div className="kanban-scroll flex gap-3 overflow-x-auto pb-2">
                {DEAL_STAGES.map((stage) => {
                    const deals = pipeline.by_stage[stage] ?? [];
                    return (
                        <div
                            key={stage}
                            className="min-w-0 flex-1 rounded-lg border border-gray-200 bg-gray-50 p-3 dark:border-gray-700 dark:bg-gray-800/50"
                        >
                            <div className="mb-2 flex items-center gap-1.5 border-b-2 border-gray-200 pb-2 text-xs font-semibold tracking-wider whitespace-nowrap text-gray-500 uppercase dark:border-gray-700 dark:text-gray-400">
                                <Badge variant={STAGE_VARIANT[stage]}>
                                    {formatStageName(stage)}
                                </Badge>
                                <span>{deals.length}</span>
                            </div>
                            {deals.length === 0 ? (
                                <p className="text-sm text-gray-500 dark:text-gray-400">No deals</p>
                            ) : (
                                deals.map((deal) => (
                                    <Link
                                        key={deal.deal_id}
                                        to={`/deals/${deal.deal_id}`}
                                        className="mb-2 block rounded-md border border-gray-200 bg-white p-3 text-sm transition-colors hover:border-indigo-600 dark:border-gray-700 dark:bg-gray-800"
                                    >
                                        <p className="font-semibold text-gray-900 dark:text-gray-100">
                                            {deal.title}
                                        </p>
                                        <p className="mt-0.5 font-semibold text-indigo-600">
                                            {formatCurrency(deal.value)}
                                        </p>
                                    </Link>
                                ))
                            )}
                        </div>
                    );
                })}
            </div>

            {/* Pipeline summary row */}
            <div className="mt-4 flex gap-6">
                <div className="flex flex-col">
                    <span className="text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
                        Total pipeline
                    </span>
                    <span className="font-heading text-lg font-semibold text-gray-900 dark:text-gray-100">
                        {formatCurrency(pipeline.total_value)}
                    </span>
                </div>
                <div className="flex flex-col">
                    <span className="text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
                        Won
                    </span>
                    <span className="font-heading text-lg font-semibold text-green-500">
                        {formatCurrency(pipeline.won_value)}
                    </span>
                </div>
                <div className="flex flex-col">
                    <span className="text-xs font-semibold tracking-wide text-gray-500 uppercase dark:text-gray-400">
                        Lost
                    </span>
                    <span className="font-heading text-lg font-semibold text-red-500">
                        {formatCurrency(pipeline.lost_value)}
                    </span>
                </div>
            </div>
        </div>
    );
}
