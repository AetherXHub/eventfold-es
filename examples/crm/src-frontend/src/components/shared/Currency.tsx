import { formatCurrency } from "@/lib/format";

interface CurrencyProps {
    cents: number;
    className?: string;
}

export default function Currency({ cents, className = "" }: CurrencyProps) {
    return <span className={className}>{formatCurrency(cents)}</span>;
}
