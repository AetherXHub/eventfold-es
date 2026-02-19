import { useEffect, type ReactNode } from "react";

interface ModalProps {
    open: boolean;
    title: string;
    onClose: () => void;
    children: ReactNode;
}

export default function Modal({ open, title, onClose, children }: ModalProps) {
    useEffect(() => {
        if (!open) return;

        function handleKeyDown(e: KeyboardEvent) {
            if (e.key === "Escape") onClose();
        }

        document.addEventListener("keydown", handleKeyDown);
        return () => document.removeEventListener("keydown", handleKeyDown);
    }, [open, onClose]);

    if (!open) return null;

    return (
        <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/45"
            onClick={onClose}
        >
            <div
                className="w-[90%] max-w-md rounded-xl bg-white p-6 shadow-2xl dark:bg-gray-800"
                onClick={(e) => e.stopPropagation()}
            >
                <div className="mb-4 flex items-center justify-between">
                    <h3 className="font-heading text-lg font-semibold text-gray-900 dark:text-gray-100">
                        {title}
                    </h3>
                    <button
                        type="button"
                        onClick={onClose}
                        className="px-1 text-xl leading-none text-gray-400 transition-colors hover:text-gray-600 dark:hover:text-gray-200"
                    >
                        {"\u00d7"}
                    </button>
                </div>
                {children}
            </div>
        </div>
    );
}
