# EventFold CRM Demo

A production-quality Tauri v2 desktop CRM application demonstrating every
eventfold-es feature in a realistic sales-pipeline domain.

## Features Demonstrated

| eventfold-es Feature | CRM Usage |
|----------------------|-----------|
| `Aggregate` trait | Contact, Company, Deal, Task, Note (5 aggregates) |
| `AggregateStore` builder | Projections, PM, aggregate types, idle timeout |
| `CommandBus` | All frontend writes route through the bus |
| `Projection` | PipelineSummary (dashboard), ActivityFeed (timeline) |
| `ProcessManager` | DealTaskCreator (auto-task on deal stage change) |
| `CommandEnvelope` | DealTaskCreator -> Task command dispatch |
| `CommandContext` | Actor identity, correlation ID propagation |
| Idle eviction | 5-minute timeout via builder |
| Tracing | Structured logs in `cargo tauri dev` console |
| Dead-letter log | Failed PM dispatches recorded in JSONL |

## Architecture

```
Frontend (Pico CSS + vanilla JS)
    |
    | window.__TAURI__.core.invoke()
    v
Tauri Commands (src/commands.rs)
    |
    | CommandBus::dispatch()
    v
Aggregates (src/domain/*.rs)
    |
    | Events appended to eventfold JSONL logs
    v
Projections (src/projections.rs)     Process Managers (src/workflows.rs)
PipelineSummary, ActivityFeed        DealTaskCreator -> auto-creates Tasks
```

## Domain Model

- **Contact** -- people (name, email, phone, tags, company link, archive)
- **Company** -- organizations (name, industry, website, archive)
- **Deal** -- sales opportunities with pipeline stages (Prospect -> Qualified
  -> Proposal -> Negotiation -> ClosedWon/ClosedLost)
- **Task** -- to-do items with optional due dates, linked to deals/contacts
- **Note** -- freeform text annotations linked to contacts/deals

## Prerequisites

- Rust 1.80+
- Tauri CLI v2: `cargo install tauri-cli@^2`
- Linux: `gtk3-devel glib2-devel webkit2gtk4.1-devel` (or equivalent)
- macOS: Xcode Command Line Tools
- Windows: WebView2

## Running

```bash
cargo tauri dev --manifest-path examples/crm/Cargo.toml
```

## Testing

```bash
# Unit tests (domain aggregates, projections, process manager)
cargo test -p eventfold-crm

# Integration tests (full saga roundtrips)
cargo test -p eventfold-crm --test integration
```

## Tech Stack

- **Backend**: Rust + eventfold-es + Tauri v2
- **Frontend**: Pico CSS + vanilla JavaScript (no bundler, no framework)
- **Fonts**: Inter (body), Space Grotesk (headings) via Google Fonts
- **Theme**: Adaptive light/dark with indigo primary palette
