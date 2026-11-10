# MynaCAD Developer Guidelines: Documentation Maintenance and Change Management

**Document type:** Engineering Standard  
**Applies to:** All developers, AI coding agents, maintainers and contributors  
**Scope:** Entire MynaCAD repository, including application code, architecture, commands, UI, APIs, file formats, configuration, tests, dependencies and build systems  
**Status:** Adopted  
**Canonical location:** this file (`docs/documentation-policy.md`)

Related canonical pages:

| Subject | Canonical document |
|---|---|
| Implemented vs planned capability | [status.md](status.md) |
| System architecture | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Agent and developer rules (summary) | [CLAUDE.md](../CLAUDE.md) |
| Build, test, packaging | [BUILD_AND_DEVELOPMENT.md](BUILD_AND_DEVELOPMENT.md), [TESTING.md](TESTING.md) |
| Generated command/menu parity | [parity-checklist.md](parity-checklist.md) (`cargo xtask parity`; do not edit by hand) |
| WinUI secondary window sizing / exit | [WINDOW_SIZING.md](WINDOW_SIZING.md) |
| WinUI design tokens / density | [UI_TOKENS.md](UI_TOKENS.md) |
| WinUI window templates / components / layout rules | [UI_STANDARDS.md](UI_STANDARDS.md) |

---

## 1. Purpose

MynaCAD must maintain accurate, complete and version-controlled documentation throughout its development lifecycle.

Documentation is an integral part of software development. It must evolve whenever the implementation, architecture, supported behaviour, public interfaces or operational requirements change.

These guidelines establish a consistent process for:

- Adding new features and documenting them.
- Modifying existing features and correcting outdated instructions.
- Removing features, APIs, dependencies or architectural components.
- Refactoring the codebase without losing architectural knowledge.
- Updating tests, configuration, build instructions and developer workflows.
- Maintaining compatibility information and known limitations.
- Preventing AI-generated or manually written documentation from making unsupported claims.
- Ensuring every release has a clear and reproducible description of its capabilities.

## 2. Fundamental principles

### 2.1 Documentation must reflect reality

Documentation must describe the implementation that exists, not the implementation that developers intended to create.

Never document a feature as complete merely because:

- Its UI has been implemented.
- A command has been registered.
- A function or class exists.
- A pull request has been merged.
- A test has been written but not executed.
- An AI agent has generated the implementation.

Claims about functionality must be supported by appropriate implementation evidence and verification.

### 2.2 Every change requires a documentation-impact assessment

Before completing any change, determine whether it affects:

- User-visible functionality.
- Developer-facing APIs or interfaces.
- Architecture or component responsibilities.
- Commands, shortcuts, settings or workflows.
- Data structures, schemas or persistence.
- File import, export or compatibility.
- Build, test, packaging or deployment procedures.
- Security, performance, limitations or supported platforms.

If documentation is affected, update it in the same pull request as the implementation.

If no documentation change is necessary, record that conclusion in the pull request checklist.

### 2.3 One canonical source per subject

Each subject must have one clearly identified canonical documentation location.

For example:

- Command behaviour: canonical command reference.
- Architectural decisions: architecture decision records.
- Build and test procedures: developer documentation.
- Supported functionality: capability matrix ([status.md](status.md)).
- Released changes: changelog and release notes.
- File-format compatibility: compatibility documentation.

Other pages may link to or summarise canonical information, but must not independently maintain contradictory copies.

### 2.4 Generated and human-authored documentation are different

Generate information that can be derived deterministically from code or metadata, such as command inventories, API indexes and configuration references.

Maintain explanatory material, design rationale, tutorials, troubleshooting instructions and known limitations as reviewed documentation.

Generation tools must never silently overwrite human-authored content.

### 2.5 Preserve historical knowledge

When functionality or architecture changes, update the current documentation while retaining the history needed to understand previous decisions, releases and migrations.

Do not erase an accepted architectural decision merely because the implementation has changed. Mark the decision as superseded and link to its replacement.

---

## 3. Mandatory change-to-documentation policy

Every pull request must be assessed against the following requirements.

| Change category | Required documentation action |
|---|---|
| New feature | Document purpose, behaviour, usage, limitations and verification |
| Feature modification | Update existing descriptions, examples, screenshots and acceptance criteria where affected |
| Feature removal | Remove current usage instructions, document migration or replacement guidance, and record release impact |
| Bug fix | Update documentation if user-visible behaviour, workarounds or limitations change |
| Refactoring | Update architecture and implementation guides if responsibilities, dependencies or extension points change |
| API change | Update API reference, examples, compatibility notes and migration guidance |
| Command or shortcut change | Update command reference, aliases, prompts and interaction rules |
| UI workflow change | Update user guides, interaction specifications and relevant screenshots |
| Data model or schema change | Update schema documentation, migrations, compatibility and recovery instructions |
| Dependency change | Update dependency records, installation instructions, licensing or compatibility information as applicable |
| Build or test change | Update setup, build, debugging and test instructions |
| Configuration change | Update configuration reference, defaults, examples and migration instructions |
| Performance change | Update benchmarks or performance documentation when supported measurements or expectations change |
| Security change | Update relevant security guidance, threat assumptions and configuration requirements |
| Platform support change | Update supported-platform declarations, requirements and validation results |
| Architecture change | Update system diagrams, component boundaries, relevant ADRs and integration documentation |
| Release or deprecation | Update changelog, release notes, version compatibility and removal schedule |

A change may affect multiple categories. Developers must update every affected canonical document, not just the page that is easiest to edit.

---

## 4. Required development workflow

Documentation maintenance must be integrated into the normal software development lifecycle.

### Step 1 — Inspect before modifying

Before implementing a change:

1. Read the relevant canonical documentation.
2. Inspect the current implementation and tests.
3. Identify the actual source of truth.
4. Identify dependent features and affected documentation pages.
5. Determine whether existing documentation is already inaccurate.

Do not assume that existing documentation accurately describes the current implementation.

### Step 2 — Implement and verify the change

Modify the implementation and add or update appropriate tests.

Verify actual behaviour using the applicable test suite, manual acceptance procedure or integration environment.

Record failures and limitations honestly. Do not claim that tests passed when they were not executed.

### Step 3 — Update documentation

Update the affected canonical documentation in the same pull request.

Include relevant changes to:

- Behaviour and user workflows.
- API contracts and examples.
- Architecture and dependency relationships.
- Configuration and default values.
- Limitations and compatibility.
- Tests and verification procedures.
- Migration and upgrade instructions.

Update generated references through the repository's generation tooling rather than editing generated output manually.

### Step 4 — Validate consistency

Run the applicable documentation checks and tests.

Check for:

- Incorrect or outdated claims.
- Missing documentation for new functionality.
- References to removed features.
- Broken links and invalid examples.
- Stale diagrams or generated indexes.
- Contradictory capability statuses.
- Missing migration or deprecation guidance.

### Step 5 — Review and merge

The pull request must include a documentation-impact assessment.

Reviewers must verify that the documentation matches the implementation and that claims about completion are supported by evidence.

Documentation and code should be reviewed together. A material implementation change must not be merged with knowingly outdated canonical documentation unless an explicitly approved exception applies.

### Step 6 — Maintain release accuracy

When preparing a release, verify that the documentation describes the functionality actually included in that release.

Do not expose unreleased functionality as generally available in the documentation for a stable release.

---

## 5. Rules for adding features

Whenever a feature is added, document the following as applicable.

### Functional documentation

- Feature purpose and user problem.
- Supported behaviour and intended use.
- Entry points, commands, shortcuts and UI interactions.
- Inputs, outputs, defaults and validation rules.
- Error conditions and cancellation behaviour.
- Dependencies on other features.
- Limitations and unsupported scenarios.

### Technical documentation

- Relevant modules, interfaces and component ownership.
- Public APIs and extension points.
- Data structures and persistence changes.
- Configuration and feature flags.
- Transaction, undo/redo and state-management implications.
- Performance considerations.
- Test coverage and acceptance criteria.

### Verification

- Tests that demonstrate the behaviour.
- Known gaps in automated coverage.
- Reproducible manual verification where automation is impractical.
- Compatibility and regression tests when existing files or workflows may be affected.

Do not create empty or speculative documentation pages merely to make a feature appear complete.

## 6. Rules for modifying existing features

When behaviour changes, update every document that describes the previous behaviour.

Examples include:

- A command changes its accepted arguments or default values.
- A keyboard shortcut is reassigned.
- A panel moves or is replaced.
- A feature changes from synchronous to asynchronous execution.
- A setting changes its default.
- A file import routine begins supporting additional entity types.
- An operation changes its undo or transaction behaviour.

Review examples, diagrams, screenshots, tutorials, troubleshooting instructions and acceptance criteria.

Search the repository for references to the previous behaviour where necessary. Do not rely solely on the page most closely associated with the code.

If the old behaviour remains available through a compatibility mode or legacy API, document that distinction explicitly.

## 7. Rules for removing features

Removing a feature requires more than deleting its implementation and documentation page.

Before removal:

1. Identify dependencies, consumers and integrations.
2. Search for references in source code, documentation, tests, examples and configuration.
3. Determine whether migration or replacement guidance is required.
4. Identify persisted data or files that may depend on the feature.
5. Assess backward compatibility and release impact.

After removal:

- Remove obsolete instructions from current documentation.
- Update feature indexes and capability matrices.
- Remove or update stale examples and screenshots.
- Update API references and configuration documentation.
- Document replacements and migration procedures where applicable.
- Record the change in the changelog or release notes.
- Preserve historical documentation where needed to explain older releases.

Never delete the only record of a feature's previous behaviour if users or developers may need it to maintain or migrate existing projects.

For a breaking change, explain the old behaviour, the new behaviour, the migration steps and the affected version range.

## 8. Rules for architecture and structural changes

Whenever a component is introduced, removed, merged, split or reassigned, review the architectural documentation.

Update, where applicable:

- System context and component diagrams.
- Module and dependency boundaries.
- Data flow and execution flow.
- Ownership of state and business logic.
- Threading and concurrency assumptions.
- Error propagation and transaction boundaries.
- Rendering and resource ownership.
- Integration contracts between components.
- Build and deployment dependencies.
- Relevant ADRs and their status.

Do not create an ADR for every minor refactor. Create or update one when a decision has significant architectural consequences, affects important trade-offs or establishes a convention that future developers must understand.

If an existing ADR becomes invalid, mark it as superseded and link to the new decision. Do not silently rewrite historical rationale.

## 9. Rules for data, APIs and compatibility

Changes to persisted data, external interfaces or file handling require special care.

Document:

- Schema and data-model changes.
- Migration order and rollback limitations.
- Backward and forward compatibility where applicable.
- API signatures, accepted inputs and returned results.
- Error handling and validation.
- Serialization and deserialization behaviour.
- Supported versions and known limitations.
- Recovery procedures for incompatible or damaged data.

For file-format compatibility, distinguish opening, reading, displaying, editing, saving, exporting and round-trip preservation. These are separate capabilities and must not be represented by one broad compatibility claim.

Test representative legacy files whenever a change could affect their interpretation or preservation.

## 10. Canonical documentation ownership

Every documentation subject must have a designated owner or owning team.

The owner is responsible for keeping the canonical content accurate, reviewing material changes and resolving conflicting descriptions.

Recommended ownership:

| Documentation domain | Responsible owner |
|---|---|
| System architecture | Architecture maintainer |
| Commands and command metadata | Command-engine maintainer |
| UI and user workflows | Frontend maintainer |
| Public APIs | API owner |
| File-format compatibility | Import/export maintainer |
| Build, CI and release procedures | Build/release maintainer |
| Security guidance | Security owner |
| Capability matrix | Maintainers of the relevant features |
| User guides | Feature owner, reviewed by documentation maintainers |

Ownership does not mean only one person can update a page. It means there is a clear person or team responsible for its correctness.

If ownership is not formally assigned, the pull request author remains responsible for documentation updates associated with their change.

---

## 11. Documentation quality requirements

All maintained documentation must satisfy these requirements.

### Accuracy

- Describe current behaviour.
- Identify assumptions and limitations.
- Avoid unsupported completion or compatibility claims.
- Use the same terminology as the implementation.

### Clarity

- Write for the intended audience.
- Separate user instructions from technical explanations.
- Prefer direct, actionable language.
- Explain acronyms and specialised concepts where needed.

### Traceability

- Link important capability claims to source code, tests or reproducible evidence.
- Link architecture decisions to relevant implementation areas.
- Identify the release or version scope when behaviour differs across versions.

### Maintainability

- Avoid duplicated canonical information.
- Prefer relative links within the repository.
- Generate mechanically derivable references.
- Use stable identifiers for commands, APIs and architectural decisions.
- Avoid hard-coded line numbers or other fragile source references.

### Examples

- Use examples that work with the current implementation.
- Validate executable examples where practical.
- Mark pseudocode and illustrative snippets explicitly.
- Never present proposed functionality as working code.

### Change history

- Preserve the reason for significant decisions.
- Record breaking changes and deprecations.
- Keep release notes focused on changes relevant to users and developers.

## 12. Capability status and evidence policy

Use a controlled vocabulary for capability tracking:

- `planned` — not implemented.
- `in_progress` — implementation is underway.
- `implemented` — implementation exists and basic behaviour has been verified.
- `verified` — declared acceptance criteria pass for a defined scope.
- `partial` — some documented behaviour works, but limitations remain.
- `experimental` — available but not considered stable.
- `unsupported` — explicitly outside the supported capability scope.
- `deprecated` — still present but scheduled for removal.

A feature may have different statuses for implementation, automated testing, release availability and compatibility. Track those dimensions separately when a single status would be misleading.

Do not mark a feature `verified` solely because it compiles. Verification must match the claim being made.

[status.md](status.md) is the capability matrix. Prefer these status words when refining rows; until a row is migrated, treat the existing “Implemented” / “Planned” tables as `implemented` / `planned`.

## 13. AI coding agent requirements

All AI coding agents, including Cursor, must follow the same documentation rules as human contributors.

Before editing:

- Inspect the current implementation and canonical documentation.
- Identify the actual source of truth.
- Determine the documentation impact of the requested change.

During editing:

- Update code and documentation together.
- Reuse existing conventions and terminology.
- Regenerate derived references using approved tools.
- Avoid creating duplicate documentation systems.
- Never invent tests, execution results, APIs or compatibility guarantees.

Before reporting completion:

- Run the applicable tests and documentation checks.
- Identify any tests that could not be executed.
- Report affected documentation files.
- List known limitations and remaining work.
- Distinguish implemented work from proposed follow-up work.

An AI agent must never claim that a documentation system, test suite or feature has been completed without inspecting the resulting repository and verifying the relevant outcome.

## 14. CI enforcement and exceptions

Automated checks should enforce objective requirements:

- Documentation builds successfully.
- Required metadata is valid.
- Generated references are up to date.
- Links and anchors are valid.
- Mandatory feature documentation exists.
- Capability-to-evidence references are valid.
- Deprecated features are not presented as current.
- Required migration guidance exists for declared breaking changes.

Not every prose change can be evaluated automatically. Human review remains necessary for clarity, accuracy and completeness.

If a material documentation update cannot reasonably be completed in the same pull request, require an explicit, approved exception with a tracked follow-up issue, responsible owner and due milestone. Do not allow undocumented changes to become the normal workflow.

## 15. Pull request documentation checklist

Every pull request must answer the following questions:

- [ ] Have I assessed the documentation impact of this change?
- [ ] Does the canonical documentation accurately describe the new implementation?
- [ ] Have all affected references to old behaviour been updated?
- [ ] If a feature was removed, have obsolete instructions and indexes been updated?
- [ ] If architecture or interfaces changed, have the relevant technical documents been reviewed?
- [ ] Have configuration, data migrations, compatibility and release implications been addressed where applicable?
- [ ] Are capability statuses supported by actual evidence?
- [ ] Have generated documentation and metadata validators been run?
- [ ] Have the relevant tests been executed, with failures or omissions disclosed?
- [ ] If no documentation update was needed, have I explained why?

**Merge policy:** A material change must not be merged while its canonical documentation knowingly misrepresents the implementation, except under a documented and approved exception.

## 16. Definition of done

A change is complete only when:

1. The implementation satisfies its acceptance criteria.
2. Relevant tests have been added or updated.
3. The documentation-impact assessment is complete.
4. All affected canonical documents have been updated.
5. Generated references are current.
6. Compatibility and migration implications have been addressed.
7. Documentation validation and applicable CI checks pass.
8. Known limitations and unverified areas are disclosed.
9. The pull request provides sufficient information for future maintainers to understand the change.

These requirements apply to feature additions, modifications, removals, bug fixes, refactoring and maintenance work.

---

## 17. Final engineering rule

Documentation is part of the deliverable, not a follow-up task.

Every contributor is responsible for ensuring that the repository tells the truth about its current implementation. Every material change must leave the code, tests and canonical documentation in agreement.

The objective is not to maximise the volume of documentation. It is to maintain the smallest complete, accurate and verifiable body of documentation needed to understand, use, test, extend and maintain MynaCAD.
