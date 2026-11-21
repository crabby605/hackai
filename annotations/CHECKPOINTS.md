# Checkpoint policy

Nudge after a commit, verified red-to-green loop, long agent run, session end,
or every 10 user turns by default. Never interrupt work.

- Configure the turn floor in the repository-local, gitignored
  `.git/hackai/config.json` as `checkpoint_turn_interval`. It defaults to 10
  and must be an integer from 5 through 100.
- Cap at one checkpoint per 20 minutes or configured turn floor.
- Evidence must verify against transcript events and git; invalid evidence is
  discarded.
- Labels can only lower/flag credit, never raise it.
- Chapter titles/reflections are local-only by default and disputable.
- Red-team repository prompt injection demanding favorable labels.
