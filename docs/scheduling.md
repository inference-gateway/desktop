# Scheduled jobs and GitHub tasks

An agent can work without a chat open: a **scheduled job** runs a prompt on a cron schedule, and a **task** is a GitHub issue an agent picks up in GitHub Actions. Both live under **Settings > GitHub**, in three tabs - **General**, **Scheduling** and **Tasks**.

## Scheduled jobs

A job is a prompt with a schedule, one YAML file per job in `~/.infer/schedules/<id>.yaml`:

```yaml
id: morning-quote
name: Morning quote
cron_expression: "0 8 * * *"
prompt: "Find one inspiring quote and answer with the quote and its author."
run_once: false
```

`cron_expression` takes five-field cron (`0 9 * * 1-5` is weekdays at 9) or a descriptor such as `@daily` or `@every 30m`. `run_once: true` deletes the job after its first run, and an optional `model` picks a model other than the one in the top bar. Every run starts a fresh agent session, so a prompt must stand on its own - nothing carries over from the last run.

You can write the file by hand, or ask any chat: with scheduling on, chats have the CLI's `Schedule` tool, so "every weekday at 9, summarize my open pull requests" makes the agent create the job. The tool asks for approval before it runs.

Open chats list the jobs at the top as **Scheduled jobs (N)**: each one's name, its schedule, **one-off** for a job that runs once, its prompt, and **failed** when its last run failed - hover it for the error.

## Scheduling

- **Enable local scheduling** turns the `Schedule` tool on, and with it the scheduler of the `infer daemon` the app runs. It is saved as `tools.schedule.enabled` in `~/.infer/tools.yaml`, where the CLI reads it.
- **Backend** picks where jobs run: `local` on this computer, `github` on GitHub Actions.
- **Save config** restarts the daemon on the new settings. **Scheduler daemon** shows whether it runs, and expands to its last 200 log lines - `scheduler started` means jobs will fire, and every run logs `firing scheduled job`.

With the `local` backend, jobs fire only while the app is open. Each run is recorded twice: as a chat in the sidebar, titled with the job's prompt and holding the whole run, and as a run record in `~/.infer/schedules/runs/<session>.yaml` with the job's `id`, the run's status, when it started and finished, and its error if it failed.

### The github backend

With `github`, each job is deployed as a GitHub Actions workflow, `.github/workflows/<job-id>.yml`, in your **routines repository**, and runs there through [infer-action](https://github.com/inference-gateway/infer-action) as your GitHub App's bot, also while this computer is off.

- **Repository** names the routines repository. Left empty it is `<your login>/.routines`, created private on the first save. **Create repository** creates a missing one right away.
- Schedules fire in UTC, at most every 5 minutes. A schedule GitHub cannot express is refused when the job is saved, with the reason in the chat.
- **Open a pull request per change instead of pushing to main** turns every saved job into a pull request to merge instead of a push.
- **Pull run conversations back into local storage** has the daemon download each run's conversation from the workflow's artifacts, so runs still show as chats here. **Artifact poll interval** (`10m`), **Artifact poll initial delay** (`1m`), **Max download attempts per artifact** (`3`) and **Rate-limit backoff** (`1h`) tune how it polls.

The workflows need the GitHub CLI and the Actions secrets on the General tab.

## General

- The GitHub CLI's state: **GitHub CLI authenticated**, or what to run - `gh auth login`, after installing `gh` if it is missing.
- The Actions secrets of the routines repository, each with a dot that turns green once it is set: **App client ID** (`APP_CLIENT_ID`) and **App private key** (`APP_PRIVATE_KEY`) of your infer GitHub App, and a **Provider API key** under the variable its provider reads, such as `OPENAI_API_KEY`. **Set** sends the value straight to GitHub with `gh secret set` - the app never stores it. Rename a secret in its name box and press **Save config** to use another name. **Repository secrets** and **Actions runs** open the repository's pages on GitHub.
- **Bot identity**: **Bot name** and **Bot email**, the Git author of the commits that deploy jobs. For a GitHub App bot that is `<app-slug>[bot]` and `<user-id>+<app-slug>[bot]@users.noreply.github.com`.

## Tasks

A task is a GitHub issue an agent works on in GitHub Actions. Pick a repository - any one you can open, unrelated to the routines repository - and **Install infer-action**, which opens a pull request that adds `.github/workflows/tasks.yml`. Once it is merged:

- **New task**: a **Task title** and a **Task description**, then **Create task**, which opens the issue with `@opentask` in front of the description.
- **Run task** posts a comment that starts the workflow - pick one of the templates, such as "Can you work on this?", or write your own. **Open task** opens the issue on GitHub.
- **Tasks** lists the open issues with their pull requests beneath them, and **Runs** the workflow's last runs - click one to open it on GitHub.

See [examples/scheduled-job](../examples/scheduled-job) to schedule a job and watch it run.
