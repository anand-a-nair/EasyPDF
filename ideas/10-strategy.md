# Strategy: Taking on Adobe

Short by design. The long version is a business plan nobody reads; this is the
set of things that actually decide the outcome.

## The core insight

**Acrobat's moat is not features.** Most people already use a free reader. What
keeps Adobe entrenched is being the reference implementation ("does it open in
Acrobat?" *is* the spec), enterprise procurement and deployment tooling, legal
and compliance workflows, and ecosystem lock-in via Acrobat Sign and Document
Cloud.

So this splits into two very different fights. **Consumer and prosumer is
winnable** with a genuinely better free tool. **Enterprise is not won by
features** — it's won by certification, deployment tooling, and someone to call.
Take the first completely before touching the second.

## Things that make everyone's life easier

These are the wins that cost little and compound:

- **Open instantly, do the common ten things flawlessly.** Speed is the feature
  people feel every single time. Nothing else is felt as often.
- **No account, no upload, no nag.** The entire reason people distrust the free
  tier elsewhere. Costs us nothing to be better here.
- **Never silently corrupt a file.** Honest refusal beats quiet damage. This is
  the trust foundation everything else sits on.
- **Sane defaults, no configuration required.** The tool is used occasionally,
  under mild time pressure, by people who won't read documentation.
- **Work offline, forever, with no license check.** No phone-home, no expiry.
- **Read the formats people actually receive** — including the malformed ones
  that other tools reject.
- **Be honest about limits in the UI**, not just the docs.

## What we actually need to displace Acrobat

**1. Rendering fidelity is existential.** One document rendering wrong destroys
more trust than ten missing features. Build a comparison harness against Acrobat
output early — matching its behavior *including its bugs* is the requirement.

**2. Accessibility tagging (PDF/UA) is the strategic wedge.** The European
Accessibility Act and Section 508 make tagged PDFs a *legal obligation* for a
growing set of organizations. Acrobat is effectively the only tool that does it
well, and it's expensive. This is a mandate rather than a preference — which is
exactly the kind of ground where an open tool can take real share. Large amount
of work; highest strategic return.

**3. Features we keep forgetting Acrobat has.** True redaction (actually
removing content, not drawing boxes), PDF/A conversion and validation, preflight,
document comparison, Bates numbering, form *design*. We don't need all of them —
we need to **decide deliberately which we're conceding** and say so publicly.

**4. Trust infrastructure.** Code signing and notarization (OQ-004), a
third-party security audit before any 1.0, standards-compliant PAdES so
signatures verify in Acrobat, and a real vulnerability disclosure process. A
document tool that isn't demonstrably trustworthy has no argument.

**5. Distribution is half the battle.** Package managers, app stores, and — for
any enterprise ambition — MSI plus Intune/group-policy templates. Being good is
not the same as being reachable.

## The tension we have to hold

**"Ultimate PDF tool" and "15 MB, no bloat" are in direct conflict at the
extreme.** VLC is ~50 MB installed today; it didn't stay tiny, it stayed *fast*.

Resolution, and this governs the roadmap: **budget the experience, not the
feature count.** Startup time, memory, and responsiveness stay sacred and keep
their veto. Installed size becomes a soft target met through **optional modules**
— OCR, accessibility tooling, preflight download on demand rather than shipping
in the base installer.

## The thing that actually kills projects

VLC is free to users. It was never free to *build*: VideoLAN is a nonprofit, and
VideoLabs is a commercial arm whose consulting work pays core developers. Adobe
has hundreds of engineers on Acrobat. Unfunded volunteer effort does not
out-execute that indefinitely — the usual death is the maintainer getting a
demanding job, not a technical failure.

Compatible funding paths, none of which require charging users:

- **Consulting/contract work** on the VideoLabs model — proven, strongest option
- **Public-interest grants** — NLnet and the Sovereign Tech Fund both fund
  exactly this category of local-first infrastructure. Realistic money.
- **Enterprise support contracts** — free software, paid SLA and deployment help
- **Donations and sponsorship** — real, rarely sufficient alone

Set this up while enthusiasm is high, not after burnout has started.

**Already foreclosed, deliberately:** Apache-2.0 with no CLA means no later
dual-licensing or open-core pivot without every contributor's agreement. Right
call for this project — but it's a door that is now closed, not ajar.

## How we win, in one line

Not by matching Acrobat feature for feature. By being better at the 90% people
do every week, honest about the 10% we don't do, and never asking anyone to sign
in. That's the VLC playbook, and it's the only one that has worked.
