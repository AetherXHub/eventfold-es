import type { ReactNode } from "react";

const VARIANT_CLASS: Record<string, string> = {
    prospect: "bg-indigo-500",
    qualified: "bg-violet-500",
    proposal: "bg-amber-500",
    negotiation: "bg-orange-500",
    won: "bg-green-500",
    lost: "bg-red-500",
    tag: "bg-indigo-600",
    system: "bg-slate-500",
};

interface BadgeProps {
    variant: string;
    children: ReactNode;
    className?: string;
}

export default function Badge({ variant, children, className = "" }: BadgeProps) {
    const bg = VARIANT_CLASS[variant] ?? "bg-slate-500";

    return (
        <span
            className={`inline-block rounded-full px-2 py-0.5 text-[0.68rem] font-semibold tracking-wide text-white uppercase ${bg} ${className}`}
        >
            {children}
        </span>
    );
}
