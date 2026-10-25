#!/usr/bin/env bash
# Install the optional Japanese font for compile-time catalog subsetting.
# Font generation itself remains offline and deterministic.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
destination="$root/subsystems/graphics/fonts/SourceHanSansJP-Regular.otf"
# Same Source Han Sans 2.005 source revision as the vendored SC variant.
revision="a4f7cf94edfbf1d7ffbdfc4841de276358bd7e0f2"
url="https://raw.githubusercontent.com/adobe-fonts/source-han-sans/$revision/OTF/Japanese/SourceHanSansJP-Regular.otf"

if [[ -s "$destination" ]] && [[ "$(head -c 4 "$destination")" == OTTO ]]; then
  echo "Japanese Source Han Sans font already present"
  exit 0
fi

temporary="$(mktemp "${destination}.XXXXXX")"
trap 'rm -f "$temporary"' EXIT
curl --fail --location --silent --show-error --retry 3 "$url" --output "$temporary"
if [[ "$(head -c 4 "$temporary")" != OTTO ]]; then
  echo "Downloaded Japanese font is not a valid OpenType/CFF file" >&2
  exit 1
fi
mv "$temporary" "$destination"
echo "Installed Source Han Sans JP from $revision"
