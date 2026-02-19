interface SectionHeaderProps {
    title: string;
    count: number;
    onAdd?: () => void;
}

export default function SectionHeader({ title, count, onAdd }: SectionHeaderProps) {
    return (
        <h2 className="font-heading flex items-center gap-2 text-xl font-semibold text-gray-900 dark:text-gray-100">
            {title}
            <span className="text-gray-400">({count})</span>
            {onAdd && (
                <button
                    type="button"
                    onClick={onAdd}
                    className="ml-1 inline-flex h-6 w-6 items-center justify-center rounded-full border border-gray-200 bg-transparent align-middle text-sm font-semibold text-indigo-600 transition-colors hover:border-indigo-600 hover:bg-indigo-50 dark:border-gray-700 dark:hover:bg-indigo-950/20"
                >
                    +
                </button>
            )}
        </h2>
    );
}
