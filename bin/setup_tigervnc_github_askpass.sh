#!/usr/bin/env bash
set -euo pipefail

cd /data_parallel/PreStackPro/share/nickc/tigervnc

cat > /tmp/gh-askpass-tigervnc.sh << 'EOS'
#!/usr/bin/env bash
set -euo pipefail
prompt="$1"

if echo "$prompt" | grep -qi 'username'; then
  printf '%s' 'x-access-token'
elif echo "$prompt" | grep -qi 'password'; then
  : "${GITHUB_OWNER:?Set GITHUB_OWNER}"
  : "${GITHUB_REPO:?Set GITHUB_REPO}"
  ssh nickc@birdsurvey.hopto.org "~/bin/ghapp-token ${GITHUB_OWNER} ${GITHUB_REPO}"
else
  printf ''
fi
EOS

chmod 700 /tmp/gh-askpass-tigervnc.sh

export GITHUB_OWNER=nickcrabtree
export GITHUB_REPO=tigervnc
export GIT_ASKPASS=/tmp/gh-askpass-tigervnc.sh

echo "tigervnc GitHub App askpass configured in this shell. You can now run git pull / git push."
