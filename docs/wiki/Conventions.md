# Conventions

- **Clean room.** No proprietary CAD fonts, hatch patterns, linetypes, templates, icons or help text. No GPL/LGPL/AGPL code. Every file asset has a row in `ATTRIBUTION.md`.
- **Docs reflect reality.** Documentation separates implemented from planned; never claim a feature works without an implementation and a verified test. English only, technical, no marketing.
- **One canonical source per subject.** Generated indexes are regenerated, not edited.
- **Quality gate** before every commit: `cargo xtask ci`.
- **Git.** Add files by name; never `git add -A`; the default branch is `main`.
