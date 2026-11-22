# Scheduled job

An agent runs on a schedule with no chat open: you turn local scheduling on, drop one job into `~/.infer/schedules/`, and within a minute the app's daemon fires it, records the run and adds the run's chat to the sidebar. The job is a one-off, so it fires once and goes away - see [Scheduled jobs and GitHub tasks](../../docs/scheduling.md) for jobs that repeat and for the `github` backend.

## What you need

- `task` and this checkout. Nothing else - the job runs on this computer, on the model picked in the top bar.

## Try it token-free

From the checkout:

```bash
DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=examples/scheduled-job/scenarios.yaml task dev
```

The run's reply comes from [scenarios.yaml](scenarios.yaml). For a real run, start the app normally with a model picked in the top bar - the agent then answers the job's prompt itself.

## Set up

1. Under **Settings > GitHub > Scheduling**, tick **Enable local scheduling**, leave **Backend** on `local` and press **Save config**. The daemon restarts on the new setting - expand **Scheduler daemon** and its log shows `scheduler started`.
2. Schedule the job:

   ```bash
   task up     # scheduled-hello in ~/.infer/schedules
   ```

The job is one file, [job.yaml](job.yaml), copied to `~/.infer/schedules/scheduled-hello.yaml`:

```yaml
id: scheduled-hello
name: Scheduled hello
cron_expression: "@every 1m"
prompt: "Scheduled hello: greet me in one sentence and say which day it is."
run_once: true
```

## Run it

Wait a minute. The daemon picks the file up within seconds - its log reads `scheduled job registered` - and **Scheduled jobs (1)** at the top of a chat lists `Scheduled hello` with `@every 1m` and **one-off**. When the minute is up, the log reads `firing scheduled job`, then `one-off scheduled job consumed and deleted`, and the job leaves the list.

The run is recorded in two places:

- A chat titled `Scheduled hello: greet me in one sentence and say which day it is.` in the sidebar, with the agent's reply - the run's whole conversation.
- A run record in `~/.infer/schedules/runs/`, named after that chat's session:

  ```bash
  task runs   # the run record: job_id, status, started_at, finished_at
  ```

## Clean up

```bash
task down   # removes the job if it has not fired yet
```

The run record and the chat stay - delete the chat from the sidebar if you do not want it. Untick **Enable local scheduling** and press **Save config** to stop firing jobs.
