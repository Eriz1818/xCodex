# Plan mode troubleshooting

## Plan file is not updating

Check these first:

1. Confirm an active plan file exists:

```sh
xcodex plan status
```

2. If there is no active plan, create/select one:

```sh
xcodex plan open
```

3. Verify the plan base directory is writable by xcodex:
   - In TUI: `/plan settings base-dir`
   - In CLI: check `xcodex plan status`

4. If you are using a custom in-repo base directory, ensure it is a path you intentionally want xcodex to write to.

## Why are tools blocked in Plan mode?

In Plan mode, xcodex intentionally blocks file-mutation tools by default (for example patch/write/edit flows) to keep planning safe and audit-first.

- Read-only exploration tools are still allowed.
- Approving a plan does not bypass sandbox or approval policy.
- To execute changes, use the normal implementation flow (for example choose `Start Implementation` after planning), then continue under normal approval rules.
