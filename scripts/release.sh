#!/usr/bin/env bash
#
# Cut a TCGLense release.
#
# Prompts for a version number, bumps it in api/Cargo.toml (+ Cargo.lock) and
# web/package.json (+ package-lock.json), commits, tags `vX.Y.Z`, pushes, and
# publishes a GitHub Release. Publishing the release fires the "Release images"
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
if git rev-parse --verify --quiet "origin/$BRANCH" >/dev/null; then
  behind="$(git rev-list --count "HEAD..origin/$BRANCH")"
  if [ "$behind" -gt 0 ]; then
    die "local '$BRANCH' is $behind commit(s) behind origin/$BRANCH. Pull first."
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

# --- Confirm ---------------------------------------------------------------------
echo
bold "About to:"
echo "  1. Bump api/Cargo.toml + web/package.json to $VERSION (and lockfiles)"
echo "  2. Commit on '$BRANCH': chore(release): $TAG"
echo "  3. Tag $TAG and push the branch + tag to origin"
echo "  4. Publish GitHub Release $TAG$( $PRERELEASE && printf ' (pre-release)' ) — this builds + pushes the Docker images"
echo
read -r -p "Proceed? [y/N] " reply
[ "$reply" = "y" ] || [ "$reply" = "Y" ] || die "aborted (no changes made)."

# From here the working tree gets mutated. On any failure/abort before we finish, tell
# the user exactly how to recover so a partial release isn't a mystery.
committed=false
release_ok=false
recover() {
  $release_ok && return 0
  echo >&2
  if ! $committed; then
    red "Release $TAG aborted. The version bump is only in your working tree; discard it:"
    red "  git checkout -- api/Cargo.toml api/Cargo.lock web/package.json web/package-lock.json"
  else
    red "Release $TAG aborted after the commit. Inspect and roll back as needed:"
    red "  git log -1 --oneline ; git tag -l $TAG ; git ls-remote --tags origin $TAG"
    red "  (local only? git reset --hard HEAD~1 && git tag -d $TAG)"
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

# --- Commit, tag, push -----------------------------------------------------------
echo "-> Committing..."
git add api/Cargo.toml api/Cargo.lock web/package.json web/package-lock.json
git commit --quiet -m "chore(release): $TAG"
committed=true

echo "-> Tagging $TAG..."
git tag -a "$TAG" -m "Release $TAG"

echo "-> Pushing $BRANCH + $TAG to origin..."
git push --quiet origin "$BRANCH"
git push --quiet origin "$TAG"

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
