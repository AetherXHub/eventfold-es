import type { ReactNode } from "react";

interface CardProps {
    clickable?: boolean;
    onClick?: () => void;
    className?: string;
    children: ReactNode;
}

export default function Card({ clickable = false, onClick, className = "", children }: CardProps) {
    const base =
        "bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg p-4 shadow-sm";
    const interactive = clickable
        ? "cursor-pointer hover:border-indigo-600 hover:shadow-md transition-all"
        : "";

    return (
        <div className={`${base} ${interactive} ${className}`} onClick={onClick}>
            {children}
        </div>
    );
}
