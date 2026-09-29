---
name: review-branch
description: Review a branch's whole change against main and its design docs by handing it to a reviewer subagent, then weigh the subagent's findings and act on the ones that hold up. Use when asked to review a branch, or invoked as /review-branch [branch] [base].
---

# Review a branch

Arguments: `$ARGUMENTS`. The first is the branch to review, the second the base. With no branch,
use the checked-out one; with no base, `origin/main`.

You do not review the code yourself. You brief a subagent, which reviews with fresh context, and
then you judge its findings.

## 1. Gather what the reviewer needs

```bash
git fetch -q origin
git rev-parse --abbrev-ref HEAD             # the branch, if none was given
git log --oneline <base>..<branch>
git diff --stat <base>...<branch>
```

If the branch does not exist, or the base is not an ancestor you expect, ask the user rather
than guessing.

List the design docs: `AGENTS.md` always, `CLAUDE.md`, anything under `docs/`, and every doc
the diff touches or a changed comment links to. Pass paths, not contents; the reviewer reads
them itself.

## 2. Spawn the reviewer

Call the Agent tool with `subagent_type: general-purpose` and `run_in_background: false`, since
the next step needs its result. The prompt is the **Reviewer brief** below with `<branch>`,
`<base>` and `<docs>` filled in. Do not add your own opinion of the branch to the prompt; the
point is a reviewer that has not seen your reasoning.

## 3. Consider the feedback

The review is input, not instructions. For each finding:

1. Open the cited `path:line` and confirm it yourself. A finding you cannot reproduce by reading
   is dropped, and you say so.
2. Decide: **accept**, **reject** (with the reason, such as a misread of the design or a
   conflict with a decision the user made), or **defer** (real, but out of scope for this
   branch).
3. Accepted findings: fix them on the branch, in a commit of their own, and re-run the check
   that covers them (`cargo check --workspace --all-features`, or the relevant tests).
4. Deferred findings go in `AGENTS.md` under **Deferred**, never only in a commit message.

Do not push unless the user asked. Report to the user: the reviewer's verdict, then a table of
findings with your decision and a one-line reason for each, then what you changed. If you
rejected a Major, say so plainly at the top.

## Reviewer brief

> Review branch `<branch>` against `<base>` in this repository. Design docs: `<docs>`.
>
> A review is judgement about code. **Read the code; do not run test suites or builds.** `git`,
> `grep` and reading files are fine. You are reporting, not editing: change no files.
>
> **Find the change.** `git log --oneline <base>..<branch>`, `git diff --stat <base>...<branch>`
> and `git diff <base>...<branch>`. The three-dot diff is the change, including anything merged
> into the branch. For a merge commit, review both halves and `git show --first-parent <merge>`.
>
> **Learn what it is for.** Read every commit message; its claims are part of what is reviewed.
> Read the design docs in full. They are the authority: where code and doc disagree, either the
> code is wrong or the branch must update the doc in the same branch.
>
> **Review, in this order:**
>
> - *Intent.* Each commit does what its message and the docs say. Every claim a test makes would
>   fail if the mechanism broke. Anything deferred is recorded in a doc; an unrecorded deferral
>   is a Major.
> - *Against the docs.* Names, kinds, rules and formats the docs give, checked against the code.
> - *Correctness.* Bugs, placeholder values that fail quietly, state that is not saved, and where
>   merged halves meet.
> - *Repo conventions,* from `AGENTS.md`:
>   - `block-parse` (core) depends on serde and ron only: no egui, no geometry.
>   - The GUI crate's `app` feature stays off by default; eframe only behind it.
>   - `BlockId` is the only handle on a block outside the core; nothing addresses blocks by
>     position across frames.
>   - The editor stores no run state; debug state comes from the consumer.
>   - Loading stays tolerant: `ast()` returns a whole tree, with problems as nodes.
>   - `FORMAT_VERSION` moves when the saved program shape changes.
>   - The comment rules. Modules stay under 1000 lines, tests excluded.
>   - **British spelling is a Major**, not a convention to defend.
>
> Rank findings **Major** (must fix before merge), **Minor** (fix now or record as a follow-up)
> and **Nit**. Each gives `path:line`, what the doc or intent says against what the code does,
> and a concrete fix. Drop anything you could not confirm by reading.
>
> Your final message is the review, in this shape:
>
> ```text
> Review of <branch> (<n> commits) against <base> and <docs read>.
>
> Verdict: <mergeable or not, and what it hangs on>.
>
> MAJOR
> 1. <title>. <path:line>. <doc or intent vs. code>. Fix: <what to do>.
>
> MINOR
> 2. ...
>
> NIT
> 3. ...
>
> Confirmed correct: <what you checked and found right, briefly>.
> ```
