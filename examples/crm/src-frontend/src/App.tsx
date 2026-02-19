import { HashRouter, Routes, Route, Navigate, Outlet } from "react-router-dom";
import Sidebar from "@/components/layout/Sidebar";
import Dashboard from "@/components/dashboard/Dashboard";
import ContactList from "@/components/contacts/ContactList";
import ContactDetail from "@/components/contacts/ContactDetail";
import CompanyList from "@/components/companies/CompanyList";
import CompanyDetail from "@/components/companies/CompanyDetail";
import DealBoard from "@/components/deals/DealBoard";
import DealDetail from "@/components/deals/DealDetail";
import TaskList from "@/components/tasks/TaskList";
import NoteList from "@/components/notes/NoteList";

function AppShell() {
    return (
        <div className="flex min-h-screen">
            <Sidebar />
            <main className="ml-[220px] min-w-0 flex-1 bg-white p-6 dark:bg-gray-900">
                <Outlet />
            </main>
        </div>
    );
}

export default function App() {
    return (
        <HashRouter>
            <Routes>
                <Route element={<AppShell />}>
                    <Route index element={<Navigate to="/dashboard" replace />} />
                    <Route path="dashboard" element={<Dashboard />} />
                    <Route path="contacts" element={<ContactList />} />
                    <Route path="contacts/:id" element={<ContactDetail />} />
                    <Route path="companies" element={<CompanyList />} />
                    <Route path="companies/:id" element={<CompanyDetail />} />
                    <Route path="deals" element={<DealBoard />} />
                    <Route path="deals/:id" element={<DealDetail />} />
                    <Route path="tasks" element={<TaskList />} />
                    <Route path="notes" element={<NoteList />} />
                </Route>
            </Routes>
        </HashRouter>
    );
}
