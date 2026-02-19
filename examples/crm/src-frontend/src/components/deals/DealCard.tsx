import { useNavigate } from "react-router-dom";
import type { DealWithId } from "@/api/types";
import { useAdvanceDeal, useWinDeal, useLoseDeal } from "@/api/mutations";
import { formatCurrency } from "@/lib/format";
import Card from "@/components/shared/Card";

interface DealCardProps {
    deal: DealWithId;
}

export default function DealCard({ deal }: DealCardProps) {
    const navigate = useNavigate();
    const advanceDeal = useAdvanceDeal();
    const winDeal = useWinDeal();
    const loseDeal = useLoseDeal();

    const showAdvance =
        !deal.closed &&
        deal.stage !== "Negotiation" &&
        deal.stage !== "ClosedWon" &&
        deal.stage !== "ClosedLost";
    const showWinLose = !deal.closed;

    return (
        <Card clickable onClick={() => navigate(`/deals/${deal.id}`)}>
            <p className="font-body text-sm font-semibold text-gray-900 dark:text-gray-100">
                {deal.title}
            </p>
            <p className="mt-1 text-sm font-semibold text-indigo-600">
                {formatCurrency(deal.value)}
            </p>

            {showWinLose && (
                <div className="mt-3 flex gap-2">
                    {showAdvance && (
                        <button
                            type="button"
                            onClick={(e) => {
                                e.stopPropagation();
                                advanceDeal.mutate({ id: deal.id });
                            }}
                            className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-indigo-600 transition-colors hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                        >
                            Advance
                        </button>
                    )}
                    <button
                        type="button"
                        onClick={(e) => {
                            e.stopPropagation();
                            winDeal.mutate({ id: deal.id });
                        }}
                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-green-500 transition-colors hover:border-green-500 hover:bg-green-50 dark:border-gray-700 dark:hover:bg-green-950/20"
                    >
                        Win
                    </button>
                    <button
                        type="button"
                        onClick={(e) => {
                            e.stopPropagation();
                            loseDeal.mutate({ id: deal.id });
                        }}
                        className="rounded-md border border-gray-200 bg-transparent px-3 py-1.5 text-xs font-medium text-red-500 transition-colors hover:border-red-500 hover:bg-red-50 dark:border-gray-700 dark:hover:bg-red-950/20"
                    >
                        Lose
                    </button>
                </div>
            )}
        </Card>
    );
}
