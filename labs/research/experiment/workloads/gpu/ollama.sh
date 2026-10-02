#!/bin/sh

set -eu

# Consts
model=$1
seconds=${2%.*}
think_ms=$3
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
prompt_file="$script_dir/prompts.txt"

ollama serve >/dev/null 2>&1 &
until ollama list >/dev/null 2>&1; do sleep 1; done

ollama pull "$model" >/dev/null

end=$(( $(date +%s) + seconds ))
while [ "$(date +%s)" -lt "$end" ]; do
    ollama run "$model" "$(cat "$prompt_file")" >/dev/null
    sleep "$(awk "BEGIN { print $think_ms / 1000 }")"
done
