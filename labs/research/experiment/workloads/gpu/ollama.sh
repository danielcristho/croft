#!/bin/sh

set -eu

model=$1
seconds=${2%.*}
think_ms=$3

ollama serve >/dev/null 2>&1 &
until ollama list >/dev/null 2>&1; do sleep 1; done

ollama pull "$model" >/dev/null

end=$(( $(date +%s) + seconds ))
while [ "$(date +%s)" -lt "$end" ]; do
    ollama run "$model" "Explain in three sentences how a GPU schedules warps." >/dev/null
    sleep "$(awk "BEGIN { print $think_ms / 1000 }")"
done
