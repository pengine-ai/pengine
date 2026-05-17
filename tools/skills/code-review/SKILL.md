---
name: code-review
description: In-depth code review using git_diff and file-reading tools. Covers correctness, safety, style, and performance.
tags: [git, review, diff, refactor]
hint_allow_substrings: [review, diff, refactor, "pull request", "code review", "code change"]
---

# Code Review

Use the git and file tools to gather real data before commenting — never guess at the content of changes.

## Tool order

1. **Get the diff** — choose the right scope:
   - Changes since last commit (staged + unstaged): `git_diff`
   - Unstaged working-tree only: `git_diff_unstaged`
   - Specific branch or range: `git_diff` with `target` set to e.g. `main`
   - Single file: `git_diff` with `path` argument
   - **Do NOT use `git_status` alone** — it shows file names only, not the content of changes.

2. **Scan recent history** (optional): `git_log` with `max_count=10` for context on intent.

3. **Read key files** (optional): `read_text_file` when you need surrounding context for a changed function or type.

## Output format

Structure every review with these four sections — skip any that have nothing to say:

**Summary** — one sentence: what the diff does and why.

**Strengths** — patterns worth keeping: clear naming, good error handling, tests, clean separation.

**Issues** — problems that should be fixed before merging:
- Logic bugs or off-by-one errors
- Panics / unwraps on untrusted input
- Missing error propagation
- Broken invariants (see CLAUDE.md for project-specific ones)
- Security issues (injection, exposed secrets, unvalidated input)

**Suggestions** — improvements that are not blocking:
- Refactor opportunities
- Missing tests for new code paths
- Doc / comment gaps
- Token or performance notes if relevant

## Scope discipline

Only review what changed in the diff. Do not comment on files not shown in the diff.
If the diff is large (> 500 lines), focus on the highest-risk sections first.
