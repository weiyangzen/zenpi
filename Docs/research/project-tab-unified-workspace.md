# Project Tabs and Unified Workspace

Zenpi's top navigation is project-scoped, not feature-scoped. A tab is a
project context and owns its session, transcript, working directory, tool
context, approval mode, layout, and metadata. Selecting a tab therefore
changes the complete working context.

Goal, Learn, Review, and Session are projections inside the selected project.
They are commands or panes in one workspace, and must never be rendered or
persisted as peer top-level tabs. The project strip is the only tab strip.

The intended shape is:

```text
[default] [api] [web] [+]

active project workspace
  Conversation | Goal | Learn | Review | Session | Resources | Gantt
```

This contract is the rationale for ZP-013 and is independent of the legacy
`TabId` pane-preset identifiers retained for migration compatibility.
