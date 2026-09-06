# CONTEXT.md

Dynamic memory carried between sessions — read and written only when the user explicitly instructs it, never proactively. Typical trigger: a fix didn't work, and the user asks to investigate why and record the finding here, so a future session (told to check this file) doesn't repeat the same failed approach ("no fix upon fixes").

Prune entries once they're resolved or no longer relevant — this file is meant to stay short and current, not become a permanent log (for that, git history is authoritative).

## Format

Each entry:

```
## <short title of the issue>
Tried: <what was attempted>
Result: <what happened instead of the expected fix>
Why: <root cause, once known>
Do instead: <what a future session should try, or avoid>
```

## Example

<!--
## Search pagination crash past 10 results
Tried: increasing the page size limit in the API route.
Result: crash persisted — the limit wasn't the cause.
Why: the frontend paginator assumes a zero-indexed cursor, but the API returns a
one-indexed cursor past page 1. The off-by-one was masked below 10 results because
the first page never exercises the cursor math.
Do instead: fix the cursor indexing in src/search/paginator.ts, not the API limit.
-->
