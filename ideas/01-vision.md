# Vision

## One sentence

A PDF tool that opens instantly, does the ten things people actually need, and
never asks you to sign in.

## The problem

The PDF tool market is split badly. On one side, Adobe Acrobat and its imitators:
capable, but subscription-gated, cloud-coupled, slow to launch, and increasingly
built around upsell rather than use. On the other, the free tier: web uploaders
that want your document on their server, and desktop tools that are either
single-purpose or abandoned.

There is no default answer to "I need to sign this PDF and send it back" that is
fast, local, free, and trustworthy.

## Who it's for

- People who receive a PDF, need to do one thing to it, and want that to take
  fifteen seconds.
- People who cannot upload the document — legal, medical, financial, HR.
- People on modest hardware, where a 200 MB Electron app is a real cost.

## Principles

**Local by default.** No account, no telemetry, no network call the user didn't
ask for. A document opened in EasyPDF does not leave the machine. This is a
correctness property, not a marketing line — it should be verifiable by watching
the process's sockets.

**Fast enough to be unremarkable.** The app should open before you notice it
opening. Perceived speed is the feature; see [04-performance-budget.md](04-performance-budget.md).

**Boring, obvious UI.** The tool is used occasionally by people under mild time
pressure. Discoverability beats density. No feature that requires a tutorial.

**Correctness over coverage.** A PDF that EasyPDF cannot handle should say so,
clearly, and not silently corrupt the file. Round-tripping a document without
changing bytes we didn't intend to change is a hard requirement — this is where
most PDF tools quietly fail.

**Restraint is a feature.** Every addition costs startup time, binary size,
surface area, and attention. The performance budget has veto power over the
feature list.

## Non-goals

Stating these now, so they can be pointed at later when someone proposes them:

- **Not a cloud service.** No sync, no accounts, no server-side processing.
- **Not an Acrobat clone.** We are not chasing prepress, redaction certification,
  Bates numbering, or the long tail of enterprise workflow features.
- **Not a full text editor.** Paragraph-level reflow of existing PDF body text is
  a phase-3 experiment with modest ambitions, not a launch promise. See
  [06-features.md](06-features.md) for why this is genuinely hard.
- **Not extensible via plugins**, at least not in v1. A plugin API is a permanent
  constraint on internals and a security surface. Revisit once the core is stable.
- **Not a PDF creator/office suite.** We edit PDFs; we don't author documents.

## What success looks like

Someone needs to fill and sign a form. They open EasyPDF, do it, and close it,
without ever reading documentation, creating an account, or waiting for a splash
screen. They don't think about the tool at all. That's the goal.
