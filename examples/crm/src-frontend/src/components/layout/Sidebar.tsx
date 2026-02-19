import { NavLink } from "react-router-dom";
import ThemeToggle from "@/components/layout/ThemeToggle";

const NAV_LINKS = [
    { to: "/dashboard", label: "Dashboard" },
    { to: "/contacts", label: "Contacts" },
    { to: "/companies", label: "Companies" },
    { to: "/deals", label: "Deals" },
    { to: "/tasks", label: "Tasks" },
    { to: "/notes", label: "Notes" },
] as const;

export default function Sidebar() {
    return (
        <aside className="fixed top-0 left-0 flex h-full w-[220px] flex-col bg-indigo-950 text-indigo-200">
            <div className="px-5 pt-6 pb-4">
                <span className="font-heading text-base font-semibold text-white">
                    EventFold CRM
                </span>
            </div>

            <nav className="flex-1">
                {NAV_LINKS.map(({ to, label }) => (
                    <NavLink
                        key={to}
                        to={to}
                        className={({ isActive }) =>
                            `block border-l-[3px] px-5 py-2.5 text-sm font-medium transition-all ${
                                isActive
                                    ? "border-l-indigo-400 bg-indigo-500/15 font-semibold text-white"
                                    : "border-transparent hover:bg-white/5 hover:text-white"
                            }`
                        }
                    >
                        {label}
                    </NavLink>
                ))}
            </nav>

            <div className="border-t border-white/10 px-5 py-4">
                <ThemeToggle />
            </div>
        </aside>
    );
}
