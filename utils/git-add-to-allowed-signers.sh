#!/usr/bin/env sh
# Add given SSH public key to allowed_signers, using git's user.email.

set -e

echo "Making sure we're in repository root..."
[ -d .git ] || exit 1
[ -f utils/git-configure-signers.sh ] || exit 2

echo "Checking pubkey in '$1'..."

PUBKEY="$1"
[ -f "$PUBKEY" ] || exit 3

echo "Using Pubkey $PUBKEY, content:"
PUBKEY_DATA="$(cat "$PUBKEY")"
echo "$PUBKEY_DATA"
echo
echo "Is that OK? Newline to proceed, Ctrl+C to abort."
read

FILE="$(pwd)/allowed_signers"
echo "$(git config --get user.email) namespaces=\"git\" $PUBKEY_DATA" >> "$FILE"

echo "DONE"

