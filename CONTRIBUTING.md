# Contributing to EasyPDF

The project is in its design phase. That makes this an unusually good time to
contribute: the decisions are still soft.

## Start here

1. [ideas/01-vision.md](ideas/01-vision.md) — what this is, and what it refuses
   to become. The non-goals are as load-bearing as the goals.
2. [ideas/03-tech-decisions.md](ideas/03-tech-decisions.md) — the stack, with
   rationale and rejected alternatives.
3. [ideas/08-open-questions.md](ideas/08-open-questions.md) — genuinely
   undecided things. The most useful place to weigh in today.

## What's most useful right now

- **Answers to open questions**, especially OQ-001 (PDFium build/distribution)
  and OQ-004 (code signing) — both block Phase 0.
- **Challenges to the design.** If an assumption in `ideas/` is wrong, saying so
  now is cheap. Saying so after implementation is expensive.
- **Test corpus contributions** — real-world PDFs that break tools, particularly
  malformed ones. See [ideas/04-performance-budget.md](ideas/04-performance-budget.md).

## Ground rules

**The performance budget is binding.** A change that pushes past a budget in
[ideas/04-performance-budget.md](ideas/04-performance-budget.md) needs either an
offsetting saving or an explicit decision to move the budget. "It's only a few
hundred KB" is how every bloated application got that way.

**New dependencies need justification.** Weight, transitive tree, license, and
maintenance status. Anything in the parsing or crypto path gets extra scrutiny.

**No GPL or AGPL dependencies.** Not ideological — adopting one silently
relicenses the whole project. See [ideas/09-licensing.md](ideas/09-licensing.md).

**Security-relevant changes get a security review.** Anything touching parsing,
the sandbox boundary, signatures, or encryption. The threat model in
[ideas/07-security.md](ideas/07-security.md) is the reference.

**Document the why.** For anything non-obvious, add or update a doc in `ideas/`
in the same change. In two years the reasoning will be worth more than the code.

## Licensing of contributions

Contributions are licensed under Apache-2.0, which includes a patent grant from
each contributor. No CLA or copyright assignment is required — contributors
retain their own copyright. (Governance is OQ-006 and open to discussion.)

Source files carry:

```rust
// Copyright 2026 EasyPDF Contributors
// Licensed under the Apache License, Version 2.0.
// See the LICENSE file in the project root for details.
```

## Reporting security issues

Please don't open a public issue for a vulnerability. A private disclosure
channel will be documented in `SECURITY.md` before the first release; until
then, contact the maintainer directly.
