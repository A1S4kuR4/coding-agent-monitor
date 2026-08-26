# Security Policy

## Supported versions

Until the first stable release, security fixes are provided on the latest
commit of the default branch only.

## Reporting a vulnerability

Please do not open a public issue for a vulnerability that could expose local
coding-agent logs, usage data, credentials, or permit command execution.

Use GitHub's **Private vulnerability reporting** feature in this repository's
Security tab. Include the affected version or commit, reproduction steps,
impact, and any suggested mitigation. If private reporting has not yet been
enabled by the repository owner, open a public issue containing no sensitive
details and ask the maintainer to establish a private contact channel.

## Security model

- Usage collection runs locally and passes `--offline` to ccusage.
- The frontend receives normalized aggregate data, not raw agent logs.
- No telemetry or cloud account is required.
- Sidecars are pinned and staged by scripts that verify source/package hashes.

Do not attach real agent logs, usage exports, database files, or screenshots
containing private data to issues.
