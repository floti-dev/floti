# {{NAME}}

A Floti workspace: a folder that holds repositories and the instructions every
agent working on them follows. Floti gives this file to each session started
here or in a repository under it, whichever agent runs it.

## Layout

- `workspace.yaml` is Floti's record of this workspace.
- `repos/` holds the repositories, one folder each. Clone new ones here.

## How to work here

- Work in the repository the task is about, and read its own instructions
  (its AGENTS.md or CLAUDE.md) first. They win over this file inside it.
- Make changes on a branch, not the default branch.
- Ask before anything hard to undo or seen outside this computer: pushing,
  merging, publishing, deleting or sending.
- Run the tests for what you changed before calling it done, and say what
  you did not check.
- Keep to what was asked. Mention anything else you notice instead of
  fixing it unasked.

## This file

Edit it to say what agents here should know: what the repositories are for,
how they fit together, and the rules that hold across them. A skill every
agent should have goes in `.agents/skills/<name>/SKILL.md`.
