#!/usr/bin/env bash
#
# Cut a TCGLense release.
#
# Prompts for a version number, bumps it in api/Cargo.toml (+ Cargo.lock) and
# web/package.json (+ package-lock.json), then lands the bump on `main` through a
# pull request — `main` is protected, so a direct push is rejected. It commits on a
# short-lived `chore/release-vX.Y.Z` branch, opens a PR, merges it with a merge
# commit, then tags `vX.Y.Z` on that merge commit (so GitHub's PR-based release notes
# include this release's own bump PR — see the tagging step for the off-by-one this
# avoids), and publishes a GitHub Release. Publishing the release fires the "Release images"
# workflow (.github/workflows/release.yml), which builds and pushes the Docker
# images (tcglense-api / tcglense-web / tcglense) to GHCR + Docker Hub.
#
# Run from anywhere:  ./scripts/release.sh
#
# Prerequisites: a clean working tree, and git / cargo / npm / gh on PATH with
# `gh` authenticated (`gh auth login`). The "Release images" workflow must already
# be on the repo's default branch for the release to trigger it, so land this on
# main before cutting the first release.

set -euo pipefail

# --- Locate the repo root from this script's own location (works from anywhere) ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT_DIR"

red()  { printf '\033[31m%s\033[0m\n' "$*" >&2; }
bold() { printf '\033[1m%s\033[0m\n' "$*"; }
die()  { red "Error: $*"; exit 1; }

require() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' is not installed or not on PATH."
}
require git
require cargo
require npm
require gh

# --- Preconditions ---------------------------------------------------------------
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not inside a git repository."
gh auth status >/dev/null 2>&1 || die "gh is not authenticated. Run: gh auth login"

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
BASE_BRANCH="main"  # the protected branch we cut from and merge the release PR into
if [ "$BRANCH" != "main" ]; then
  red "You are on branch '$BRANCH', not 'main'. Releases are normally cut from main."
  read -r -p "Continue on '$BRANCH' anyway? [y/N] " reply
  [ "$reply" = "y" ] || [ "$reply" = "Y" ] || die "aborted."
fi

# Refuse to run on a dirty tree — the release commit must contain only the bump.
if [ -n "$(git status --porcelain)" ]; then
  die "working tree is not clean. Commit or stash your changes first."
fi

# Make sure we're not behind the remote (missing commits that should be in the release).
echo "-> Fetching from origin..."
git fetch --quiet --tags origin
if git rev-parse --verify --quiet "origin/$BASE_BRANCH" >/dev/null; then
  behind="$(git rev-list --count "HEAD..origin/$BASE_BRANCH")"
  if [ "$behind" -gt 0 ]; then
    die "HEAD is $behind commit(s) behind origin/$BASE_BRANCH. Pull/rebase onto it first."
  fi
fi

# --- Current version (from the [package] table of api/Cargo.toml) ----------------
current_version="$(
  awk '
    /^\[/ { in_pkg = ($0 == "[package]") }
    in_pkg && /^version[[:space:]]*=/ {
      match($0, /"[^"]*"/); print substr($0, RSTART + 1, RLENGTH - 2); exit
    }
  ' api/Cargo.toml
)"
[ -n "$current_version" ] || die "could not read the current version from api/Cargo.toml."

bold "TCGLense release"
echo "  Current version: $current_version"
echo

# --- Prompt for the new version --------------------------------------------------
read -r -p "New version (X.Y.Z, without a leading 'v'): " VERSION
VERSION="${VERSION#v}"  # tolerate a pasted leading 'v'

# Strict semver: X.Y.Z with no leading zeros, plus an optional pre-release suffix of
# dot-separated non-empty identifiers (e.g. 1.2.0-rc.1). Tighter than the loose form so
# an input `npm version` would later reject (1.02.0, 1.0.0-.) is caught HERE, before any
# file is bumped.
if ! printf '%s' "$VERSION" \
  | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'; then
  die "'$VERSION' is not valid semver (expected X.Y.Z or X.Y.Z-prerelease, no leading zeros)."
fi

TAG="v$VERSION"

# A pre-release version (has a '-suffix') is flagged as a GitHub pre-release, which
# keeps the images' `latest` tag from moving to it (see release.yml).
PRERELEASE=false
case "$VERSION" in *-*) PRERELEASE=true ;; esac

# --- Refuse to clobber an existing tag -------------------------------------------
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  die "tag $TAG already exists locally."
fi
if [ -n "$(git ls-remote --tags origin "$TAG")" ]; then
  die "tag $TAG already exists on origin."
fi

# --- Refuse to clobber an existing release branch --------------------------------
RELEASE_BRANCH="chore/release-$TAG"
if git rev-parse -q --verify "refs/heads/$RELEASE_BRANCH" >/dev/null; then
  die "branch $RELEASE_BRANCH already exists locally."
fi
if [ -n "$(git ls-remote --heads origin "$RELEASE_BRANCH")" ]; then
  die "branch $RELEASE_BRANCH already exists on origin."
fi

# --- Confirm ---------------------------------------------------------------------
echo
bold "About to:"
echo "  1. Bump api/Cargo.toml + web/package.json to $VERSION (and lockfiles)"
echo "  2. Commit the bump on '$RELEASE_BRANCH' and tag it $TAG"
echo "  3. Open a PR into $BASE_BRANCH and merge it (merge commit)"
echo "  4. Push $TAG and publish GitHub Release $TAG$( $PRERELEASE && printf ' (pre-release)' ) — this builds + pushes the Docker images"
echo
read -r -p "Proceed? [y/N] " reply
[ "$reply" = "y" ] || [ "$reply" = "Y" ] || die "aborted (no changes made)."

# From here the working tree gets mutated. On any failure/abort before we finish, tell
# the user exactly how to recover so a partial release isn't a mystery.
branch_created=false
committed=false
pushed=false
merged=false
tagged=false
release_ok=false
recover() {
  $release_ok && return 0
  echo >&2
  red "Release $TAG did not finish. Current state and how to unwind it:"
  if $merged; then
    if $tagged; then
      red "  The bump was MERGED into $BASE_BRANCH and $TAG is tagged + pushed, but the"
      red "  GitHub Release wasn't published. Finish it:"
      red "    gh release create $TAG --title $TAG --generate-notes$( $PRERELEASE && printf ' --prerelease' )"
    else
      red "  The bump was MERGED into $BASE_BRANCH but $TAG was not tagged. Finish it:"
      red "    git switch $BASE_BRANCH && git pull --ff-only origin $BASE_BRANCH"
      red "    git tag -a $TAG -m 'Release $TAG' && git push origin $TAG"
      red "    gh release create $TAG --title $TAG --generate-notes$( $PRERELEASE && printf ' --prerelease' )"
    fi
  elif $pushed; then
    red "  Branch '$RELEASE_BRANCH' is on origin but not merged (no tag was pushed). Remove it:"
    red "    git switch $BASE_BRANCH"
    red "    git push origin --delete $RELEASE_BRANCH"
    red "    git branch -D $RELEASE_BRANCH"
  elif $committed; then
    red "  The bump is committed on local '$RELEASE_BRANCH' but nothing was pushed. Remove it:"
    red "    git switch $BASE_BRANCH ; git branch -D $RELEASE_BRANCH"
  elif $branch_created; then
    red "  On '$RELEASE_BRANCH' with the bump only in the working tree. Discard it:"
    red "    git checkout -- api/Cargo.toml api/Cargo.lock web/package.json web/package-lock.json"
    red "    git switch $BASE_BRANCH ; git branch -D $RELEASE_BRANCH"
  else
    red "  The version bump is only in your working tree; discard it:"
    red "    git checkout -- api/Cargo.toml api/Cargo.lock web/package.json web/package-lock.json"
  fi
}
trap recover EXIT

# The locked version of just the tcglense-api package — NOT any dependency that happens
# to share the version string (an unscoped grep would false-positive, e.g. a dep at 0.1.0).
lock_pkg_version() {
  awk '
    /^\[\[package\]\]/ { name = "" }
    /^name = / { name = $0 }
    name == "name = \"tcglense-api\"" && /^version = / {
      match($0, /"[^"]*"/); print substr($0, RSTART + 1, RLENGTH - 2); exit
    }
  ' api/Cargo.lock
}

# --- Create the release branch (main is protected; the bump merges in via PR) ----
echo "-> Creating release branch $RELEASE_BRANCH..."
git switch --quiet -c "$RELEASE_BRANCH"
branch_created=true

# --- Bump versions ---------------------------------------------------------------
echo "-> Bumping api/Cargo.toml..."
tmp="$(mktemp)"
awk -v ver="$VERSION" '
  /^\[/ { in_pkg = ($0 == "[package]") }
  in_pkg && /^version[[:space:]]*=/ && !done {
    print "version = \"" ver "\""; done = 1; next
  }
  { print }
' api/Cargo.toml > "$tmp" && mv "$tmp" api/Cargo.toml

echo "-> Updating api/Cargo.lock..."
( cd api && cargo update --quiet --package tcglense-api )

echo "-> Bumping web/package.json..."
( cd web && npm version --no-git-tag-version --allow-same-version "$VERSION" >/dev/null )

# Make sure Cargo.lock's tcglense-api entry reflects the new version, so a later
# `cargo build --locked` (CI / the Docker build) won't fail on a stale lock.
if [ "$(lock_pkg_version)" != "$VERSION" ]; then
  # Fall back to a targeted edit of the tcglense-api entry if `cargo update` didn't.
  tmp="$(mktemp)"
  awk -v ver="$VERSION" '
    /^\[\[package\]\]/ { pkg = 1; name = "" }
    pkg && /^name = / { name = $0 }
    pkg && /^version = / && name == "name = \"tcglense-api\"" {
      print "version = \"" ver "\""; pkg = 0; next
    }
    { print }
  ' api/Cargo.lock > "$tmp" && mv "$tmp" api/Cargo.lock
fi
[ "$(lock_pkg_version)" = "$VERSION" ] || die "failed to update api/Cargo.lock to $VERSION."

# --- Commit ----------------------------------------------------------------------
echo "-> Committing..."
git add api/Cargo.toml api/Cargo.lock web/package.json web/package-lock.json
git commit --quiet -m "chore(release): $TAG"
committed=true

# --- Push the branch, open a PR, merge it ----------------------------------------
# main is protected (changes must go through a PR), so the bump can't be pushed
# straight to it. The tag is created AFTER the merge, on the merge commit — NOT here
# on the pre-merge bump commit. See the tagging step below for why that matters to
# the auto-generated release notes.
echo "-> Pushing $RELEASE_BRANCH to origin..."
git push --quiet origin "$RELEASE_BRANCH"
pushed=true

echo "-> Opening pull request into $BASE_BRANCH..."
gh pr create --base "$BASE_BRANCH" --head "$RELEASE_BRANCH" \
  --title "chore(release): $TAG" \
  --body "Automated version bump to \`$TAG\` (cut by scripts/release.sh). Merging lands the bump on \`$BASE_BRANCH\`; the \`$TAG\` tag and GitHub Release follow and trigger the image build."

# Switch off the release branch so it can be deleted after the merge.
git switch --quiet "$BASE_BRANCH"

echo "-> Merging the pull request..."
# Mergeability can take a moment to compute right after the PR opens; retry briefly.
for _ in 1 2 3 4 5 6; do
  if gh pr merge "$RELEASE_BRANCH" --merge; then
    merged=true; break
  fi
  echo "   ...not mergeable yet; retrying in 3s"
  sleep 3
done
$merged || die "could not auto-merge the release PR (required checks or approvals?). Merge it in the UI, pull $BASE_BRANCH, then run: git tag -a $TAG -m 'Release $TAG' && git push origin $TAG && gh release create $TAG --generate-notes$( $PRERELEASE && printf ' --prerelease' )"

echo "-> Fast-forwarding local $BASE_BRANCH..."
git pull --quiet --ff-only origin "$BASE_BRANCH"

# --- Tag the merge commit (NOT the pre-merge bump commit) -------------------------
# Tag HEAD, which is now the merge commit of the release PR on $BASE_BRANCH. Tagging
# the bump commit instead (a *parent* of the merge commit) throws GitHub's PR-based
# `--generate-notes` off by one: this release's own "chore(release)" PR merges just
# *after* such a tag, so it drops out of the notes, while the *previous* release's
# bump PR — which merged after the previous tag — gets swept in. Tagging the merge
# commit puts this release's bump PR at the end of the range (included) and the
# previous one before its start (excluded).
echo "-> Tagging $TAG on the merge commit..."
git tag -a "$TAG" -m "Release $TAG"
git push --quiet origin "$TAG"
tagged=true

# Best-effort cleanup of the merged release branch (GitHub may auto-delete it).
git push --quiet origin --delete "$RELEASE_BRANCH" 2>/dev/null || true
git branch --quiet -D "$RELEASE_BRANCH" 2>/dev/null || true

# --- Publish the GitHub Release (triggers the image build) -----------------------
echo "-> Publishing GitHub Release $TAG..."
release_args=(--title "$TAG" --generate-notes)
$PRERELEASE && release_args+=(--prerelease)
gh release create "$TAG" "${release_args[@]}"
release_ok=true

echo
bold "Released $TAG 🎉"
repo_slug="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
if [ -n "$repo_slug" ]; then
  echo "  Release:  https://github.com/$repo_slug/releases/tag/$TAG"
  echo "  Actions:  https://github.com/$repo_slug/actions/workflows/release.yml"
fi
echo "  The 'Release images' workflow is now building tcglense-api / tcglense-web / tcglense."
