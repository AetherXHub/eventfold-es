import { useTheme } from "@/hooks/useTheme";

export default function ThemeToggle() {
    const { toggle } = useTheme();

    return (
        <button
            type="button"
            onClick={toggle}
            className="font-body w-full rounded-md border border-white/15 bg-white/10 py-2 text-xs text-indigo-200 transition-colors hover:bg-white/15"
        >
            Toggle theme
        </button>
    );
}
