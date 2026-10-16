# Utils submodule propagation

The updater advances the utils submodule on the `main` branch of each repository in
[the consumer registry](utils-consumers.json). It creates a normal commit changing
only the utils gitlink. The workflow keeps existing consumer CI and release triggers.

## Consumer setup

Each registered repository must have a utils submodule and this committed configuration:

```ini
[submodule "utils"]
    path = utils
    url = https://github.com/synthlabs/utils.git
    branch = main
```

Ordinary checkouts use the committed SHA. `branch = main` selects the upstream branch
for remote updates; it does not make builds float to a newer commit or attach local
worktrees to main. See [Git's submodule documentation](https://git-scm.com/docs/git-submodule).

To onboard a repository, add its submodule and configuration first, then add it to
the registry and the App installation. Pepo.chat is enrolled without an application
code migration. The rollout policy removes Pepo's current main protection; other
repository access permissions remain managed independently.

## Activate the GitHub App

App setup and live activation are deferred. Complete these steps before publishing
the workflow to utils/main:

1. Register an organization-owned **Synth Utils Updater** App in
   [Synth's GitHub App settings](https://github.com/organizations/synthlabs/settings/apps/new).
   Set a homepage of `https://github.com/synthlabs/utils`, disable the webhook, and grant
   repository **Contents: read and write** and **Metadata: read**. No other permissions
   or webhook subscriptions are required.
2. Install it on selected repositories: utils, blockbuster, pepo, pepo.chat, pointpit,
   scrybe, scrybe.cc, and sysmon. Generate a private key.
3. Store its Client ID and PEM key in utils Actions settings:

   ```sh
   gh variable set UTILS_UPDATER_CLIENT_ID --repo synthlabs/utils --body '<APP_CLIENT_ID>'
   gh secret set UTILS_UPDATER_PRIVATE_KEY --repo synthlabs/utils < /path/to/private-key.pem
   ```

4. Publish the consumer onboarding commits before the utils workflow. Remove Pepo's
   classic main protection as approved, before publishing its onboarding commit:

   ```sh
   gh api --method DELETE repos/synthlabs/pepo/branches/main/protection
   ```

The App token is scoped to registry consumers. The workflow's `GITHUB_TOKEN` has only
Contents read permission and handles source-repository reads. Pull requests run the
updater tests without creating an App token. Official Actions are pinned to commit SHAs.

## Validate and operate

Run the repeatable offline tests:

```sh
node --test scripts/update-utils-submodules.test.mjs
```

Inspect current consumers locally using existing `gh` authentication:

```sh
node scripts/update-utils-submodules.mjs --dry-run
```

This mode makes only GET requests; it creates no tree, commit, or reference update.
Local invocations default to dry run. Live writes require `--write`, the App token in
`GH_TOKEN`, and the App slug in `UTILS_UPDATER_APP_SLUG`.

After activation, every push to utils/main propagates a shared snapshot of the latest
main tip. Writing runs are serialized without canceling an active run. Superseded
pending runs can coalesce. Dry runs use separate concurrency so they cannot replace
a pending writing run.

Manual dispatch defaults to dry run:

```sh
gh workflow run update-utils.yml --repo synthlabs/utils --ref main -f dry_run=true
```

Retry writes explicitly:

```sh
gh workflow run update-utils.yml --repo synthlabs/utils --ref main -f dry_run=false
```

The updater skips identical or newer pins, rejects divergent histories, and retries
consumer main races up to three times with fast-forward-only updates. Individual
consumer failures do not prevent attempts for other consumers. The run summary shows
previous and resulting SHAs, statuses, commit links, and errors. Any failed consumer
fails the run; recover through manual dispatch or the next utils main push.

Existing consumer tests and releases run after update commits; they do not gate
propagation. Inspect those runs separately. To stop future propagation, disable the
workflow in GitHub Actions. There is no scheduled reconciliation or automatic rollback.
