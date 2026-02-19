interface EmptyStateProps {
    message: string;
}

export default function EmptyState({ message }: EmptyStateProps) {
    return (
        <p className="font-body py-10 text-center text-sm text-gray-400 dark:text-gray-500">
            {message}
        </p>
    );
}
