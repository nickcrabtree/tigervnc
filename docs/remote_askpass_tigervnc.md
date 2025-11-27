# Remote GitHub App askpass for tigervnc

This document records how the `tigervnc` repo is set up to push/pull from a remote VM using a GitHub App–based token, and what went wrong when it failed with a JSON/404 error.

## Goal

Allow a rehydrated VM to push/pull `nickcrabtree/tigervnc` **without storing long‑lived secrets on the VM**, by:

- Using HTTPS remotes
- Using `GIT_ASKPASS` to call back to the home workstation (`birdsurvey.hopto.org`)
- Minting a short‑lived GitHub App installation token there

## Topology

- **Local Mac**: `~/code/tigervnc` (developer checkout)
- **Remote VM**: `/data_parallel/PreStackPro/share/nickc/tigervnc` (rehydrated repo)
- **Home workstation**: `nickc@birdsurvey.hopto.org` (runs `ghapp-token` helper)

Git flow for the VM:

1. `git push` / `git pull` on the VM uses remote:
   - `origin = https://github.com/nickcrabtree/tigervnc.git`
2. Git asks for credentials via `GIT_ASKPASS=/tmp/gh-askpass-tigervnc.sh`.
3. The askpass script SSHes to the home host and runs:
   - `ssh nickc@birdsurvey.hopto.org "~/bin/ghapp-token nickcrabtree tigervnc"`
4. `ghapp-token` calls the GitHub App API and prints a short‑lived token.
5. Git uses `username = x-access-token`, `password = <token>` to complete the HTTPS op.

No PATs or SSH deploy keys live on the VM.

## Helper script on the VM

The repo now contains `bin/setup_tigervnc_github_askpass.sh`. On the VM:

```bash
cd /data_parallel/PreStackPro/share/nickc/tigervnc
./bin/setup_tigervnc_github_askpass.sh
```

This script:

- Ensures it is running in the repo root on the VM
- Writes `/tmp/gh-askpass-tigervnc.sh` with logic:
  - If Git prompts for **username** → prints `x-access-token`
  - If Git prompts for **password** → runs `ssh nickc@birdsurvey.hopto.org "~/bin/ghapp-token nickcrabtree tigervnc"` and prints the token
- Marks the askpass script executable
- Exports in the current shell:
  - `GITHUB_OWNER=nickcrabtree`
  - `GITHUB_REPO=tigervnc`
  - `GIT_ASKPASS=/tmp/gh-askpass-tigervnc.sh`

Because these are shell variables, the script must run in the **current** shell. Either:

- `source bin/setup_tigervnc_github_askpass.sh` (preferred), or
- `./bin/setup_tigervnc_github_askpass.sh` followed by manual export of the same variables.

After that, `git pull` / `git push` in that shell should use the GitHub App token.

## Symptom when the home helper is broken

When the GitHub App helper on the home workstation is not properly configured for `nickcrabtree/tigervnc`, a push from the VM looks like this:

- Git invokes askpass, which SSHes to `birdsurvey.hopto.org`
- `ghapp-token` does a `curl` to the GitHub API and gets **HTTP 404**
- The helper then tries to parse the response as JSON and crashes:

Typical error sequence from the VM:

- `curl: (22) The requested URL returned error: 404`
- Python `json.load(sys.stdin)` raises `JSONDecodeError`
- Askpass fails with:
  - `error: unable to read askpass response from '/tmp/gh-askpass-tigervnc.sh'`
- Git falls back to interactive prompts:
  - `Password for 'https://x-access-token@github.com':` (or similar)

Root cause: **`~/bin/ghapp-token nickcrabtree tigervnc` on the home host is not returning a valid JSON token** (usually because the GitHub App is not installed for that repo, or the helper is using the wrong installation ID/owner/repo mapping).

## How to diagnose

All diagnosis happens on the **home workstation** first.

1. SSH to the home host:

   ```bash
   ssh nickc@birdsurvey.hopto.org
   ```

2. Run the helper manually:

   ```bash
   ~/bin/ghapp-token nickcrabtree tigervnc
   echo $?
   ```

3. Expected behavior when working:

   - Command prints a GitHub App token (opaque string)
   - Exit code is `0`

4. Broken behavior:

   - `curl` 404 or other HTTP error
   - Python `JSONDecodeError` when parsing the response
   - Non‑zero exit code

5. If the helper is broken, **fix it on the home host first** (see next section). There is nothing to fix on the VM until this succeeds.

## Fixing the GitHub App helper on the home host

The exact implementation of `~/bin/ghapp-token` lives on the home machine, but the usual issues and remedies are:

1. **GitHub App is not installed on `nickcrabtree/tigervnc`**

   - Go to the GitHub UI for your GitHub App
   - Ensure the app is installed on the `nickcrabtree` account
   - Ensure `tigervnc` is selected among the repositories it has access to

2. **`ghapp-token` is using wrong installation IDs or URL**

   - Compare the code path used for `pspro_plugins` to the path for `tigervnc`
   - Confirm the owner (`nickcrabtree`) and repo (`tigervnc`) mapping is correct
   - Confirm the GitHub API endpoint matches current GitHub docs

3. **Re‑test helper locally**

   After any changes:

   ```bash
   ~/bin/ghapp-token nickcrabtree tigervnc
   ```

   Only when this prints a token and exits `0` should you go back to the VM.

## Retesting push from the VM

Once `ghapp-token` is fixed on the home host:

On the VM:

```bash
ssh -i ~/premierJakarta.key -o StrictHostKeyChecking=no -o UserKnownHostsFile=/tmp/HBR \
    pspuser@108.136.194.23

cd /data_parallel/PreStackPro/share/nickc/tigervnc

# Ensure helpers do not store credentials
git config --global --unset-all credential.helper || true
git config --local  --unset-all credential.helper || true

# (Re)configure askpass in the current shell
source bin/setup_tigervnc_github_askpass.sh

# Commit any changes
git status --short
# git add ...
# git commit -m "..."

# Push using GitHub App token
git push origin master
```

The push should now complete without interactive username/password prompts (other than the occasional SSH prompt when talking to the home box for the first time).

## Rehydrate considerations

- **Everything under `/data_parallel` survives reboots**. The `tigervnc` repo and `bin/setup_tigervnc_github_askpass.sh` live there and are pulled during rehydrate.
- **`/tmp` and shell environment do not survive reboots or new sessions**:
  - `/tmp/gh-askpass-tigervnc.sh` is re‑created by the helper
  - `GIT_ASKPASS`, `GITHUB_OWNER`, `GITHUB_REPO` must be re‑exported in each new shell

For a freshly rehydrated VM:

1. Run the appropriate `scripts/rehydrate_tigervnc*.sh` to rebuild dependencies and repo.
2. In any shell where you want to do Git operations for this repo:

   ```bash
   cd /data_parallel/PreStackPro/share/nickc/tigervnc
   source bin/setup_tigervnc_github_askpass.sh
   ```

3. Then run `git pull` / `git push` as normal.

If pushes suddenly start prompting for passwords again, or error with JSON/404 from Python, re‑check `~/bin/ghapp-token nickcrabtree tigervnc` on the home workstation as described above.
