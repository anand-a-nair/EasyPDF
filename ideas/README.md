# EasyPDF — Design Notes

Planning documents for EasyPDF. Code decisions get made here first, in prose,
before they get made in a directory structure.

| Doc | What it covers |
|---|---|
| [01-vision.md](01-vision.md) | What EasyPDF is, who it's for, and what it refuses to become |
| [02-architecture.md](02-architecture.md) | System shape, crate layout, process model, data flow |
| [03-tech-decisions.md](03-tech-decisions.md) | Stack decisions with rationale and rejected alternatives |
| [04-performance-budget.md](04-performance-budget.md) | Hard numbers the project is held to, and how they're enforced |
| [05-roadmap.md](05-roadmap.md) | Phased delivery plan |
| [06-features.md](06-features.md) | Feature specs: view, edit, annotate, sign, encrypt |
| [07-security.md](07-security.md) | Threat model — parsing hostile input is the core risk |
| [08-open-questions.md](08-open-questions.md) | Unresolved decisions, tracked rather than forgotten |
| [09-licensing.md](09-licensing.md) | License compatibility of every dependency |
| [10-strategy.md](10-strategy.md) | How an open tool actually takes on Adobe |
| [11-sustainability.md](11-sustainability.md) | Funding the work without charging users |
| [12-build-and-release.md](12-build-and-release.md) | Toolchain, CI matrix, versioning, release checklist |

## How to use this folder

Anything not yet ready to be code goes here. When a doc's decision gets
implemented, leave the doc in place and add a line noting where it landed —
the reasoning is worth more later than the conclusion.
