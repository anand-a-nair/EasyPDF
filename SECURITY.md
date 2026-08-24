# Security Policy

EasyPDF parses untrusted binary files that arrive by email from strangers. We
take security reports seriously, and we would rather hear about a problem
privately than read about it publicly.

## Supported versions

The project is pre-release. No versions are supported yet; this policy is in
place so a reporting channel exists from day one rather than being scrambled
together after the first report.

## Reporting a vulnerability

**Please do not open a public issue for a security vulnerability.**

Until a dedicated channel is published, report privately to the maintainer at
the email on the repository owner's profile. Once the repository is public,
GitHub Private Vulnerability Reporting will be enabled and become the preferred
route.

Please include: what you found, how to reproduce it, a sample document if one is
involved, and the version or commit affected.

**What to expect:** acknowledgement within a few days, an assessment with a
timeline, credit in the advisory unless you'd rather not be named, and a public
advisory once a fix is available.

## Scope

Especially interested in: memory-safety issues in parsing or rendering, sandbox
escapes from the worker process, signature verification bypasses, encryption
flaws, and any unexpected network activity — the app is designed to make no
outbound connections at all, so any is a bug.

Out of scope: issues in upstream PDFium (report to the Chromium project,
though we'd appreciate a heads-up), and social engineering.

## Design context

The threat model is documented in [ideas/07-security.md](ideas/07-security.md).
Two properties are load-bearing: all PDF bytes are handled in a sandboxed
worker process, and **PDF JavaScript is never executed, with no setting to
enable it**.
