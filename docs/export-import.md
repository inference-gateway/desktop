# Moving to a new machine

Settings > General > **Export / Import** moves the complete desktop state between machines: all Settings fields, sidebar projects, A2A agents, scheduled jobs, snippets, the skills registry URL and installed skills.

Export writes one portable file (JSON, YAML or TOML) in a native save dialog, or pushes it to a private GitHub repo you name (created on demand, public repos are refused). Import reads it back from either place and auto-detects the format.

Credentials (database passwords, tokens, `auth.yaml` keys) are never exported, and machine-specific paths are stored `~/`-relative so they resolve against the new machine's home. Projects that are GitHub checkouts travel as their `owner/name` rather than a full copy: Import re-clones them under the projects root and re-reads their `AGENTS.md`, keeping the file small (edited project instructions are still carried in full).
