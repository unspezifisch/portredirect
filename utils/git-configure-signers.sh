#!/usr/bin/env sh

set -e

echo "Making sure we're in repository root..."
[ -d .git ] || exit 1
[ -f utils/git-configure-signers.sh ] || exit 2

git config gpg.ssh.allowedSignersFile "$(pwd)/allowed_signers"

echo DONE

