#!/usr/bin/env bash
set -euo pipefail

directory="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
bin_dir="$HOME/.local/bin"
link="$bin_dir/tux"

if [[ ! -x "$directory/tundra-shell" ]]; then
  echo "tundra-shell was not found beside this installer" >&2
  exit 1
fi
if [[ -e "$link" || -L "$link" ]] || command -v tux >/dev/null 2>&1; then
  echo "A tux command already exists; it was not changed" >&2
  exit 1
fi

mkdir -p "$bin_dir"
ln -sT "$directory/tundra-shell" "$link"
echo "Installed $link"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "Add $bin_dir to PATH before running tux" ;;
esac
