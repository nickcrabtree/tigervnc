#!/usr/bin/env bash

cat > /tmp/gh-askpass-tigervnc.sh << 'EOS3'
#!/usr/bin/env bash
set -euo pipefail
prompt="$1"

if echo "$prompt" | grep -qi 'username'; then
  printf '%s' 'x-access-token'
elif echo "$prompt" | grep -qi 'password'; then
  : "${GITHUB_OWNER:?Set GITHUB_OWNER}"
  : "${GITHUB_REPO:?Set GITHUB_REPO}"

  cache_file="/tmp/gh-askpass-${GITHUB_OWNER}-${GITHUB_REPO}.cache"

  if [ -r "$cache_file" ]; then
    read -r cached_token < "$cache_file"
    printf '%s' "$cached_token"
    exit 0
  fi

  token="$(ssh nickc@birdsurvey.hopto.org "~/bin/ghapp-token ${GITHUB_OWNER} ${GITHUB_REPO}")"
  umask 0077
  printf '%s\n' "$token" > "$cache_file"
  printf '%s' "$token"
else
  printf ''
fi
EOS3

chmod 700 /tmp/gh-askpass-tigervnc.sh

export GITHUB_OWNER=nickcrabtree
export GITHUB_REPO=tigervnc
export GIT_ASKPASS=/tmp/gh-askpass-tigervnc.sh
git config --global user.name "Nick Crabtree"
git config --global user.email nickcrabtree@gmail.com
git config --global push.default simple

echo "Configured askpass for ${GITHUB_OWNER}/${GITHUB_REPO} (helper: $GIT_ASKPASS)"
echo "tigervnc GitHub App askpass configured in this shell. You can now run git pull / git push."
