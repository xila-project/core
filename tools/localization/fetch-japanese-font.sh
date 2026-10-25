#!/usr/bin/env bash
# Install the optional Japanese font for compile-time catalog subsetting.
# Font generation itself remains offline and deterministic.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
destination="$root/subsystems/graphics/fonts/SourceHanSansJP-Regular.otf"
# Same Source Han Sans 2.005 source revision as the vendored SC variant.
revision="a4f7cf94edfb9d7ffbdfc4841de276358bd7e0f2"
# Git blob ID of the official 4,562,224-byte JP subset OTF at that revision.
expected_blob_sha="5c11ef7a56fca42de83614aa7480304814b295d7"
url="https://raw.githubusercontent.com/adobe-fonts/source-han-sans/$revision/SubsetOTF/JP/SourceHanSansJP-Regular.otf"

valid_font() {
  [[ -s "$1" ]] &&
    [[ "$(head -c 4 "$1")" == OTTO ]] &&
    [[ "$(git hash-object "$1")" == "$expected_blob_sha" ]]
}

if valid_font "$destination"; then
  echo "Japanese Source Han Sans font already present"
  exit 0
fi

temporary="$(mktemp "${destination}.XXXXXX")"
trap 'rm -f "$temporary"' EXIT
curl --fail --location --silent --show-error --retry 3 "$url" --output "$temporary"
if ! valid_font "$temporary"; then
  echo "Downloaded Japanese font failed OpenType header or pinned Git blob integrity check" >&2
  exit 1
fi
mv "$temporary" "$destination"
sha256sum "$destination"
echo "Installed Source Han Sans JP from $revision"
