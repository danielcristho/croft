# Shell steps of the service-interaction study (RUNNING.md section 18), sourced by the justfile recipes.

# si_preflight [full]: nodes Ready and Linkerd healthy; full also prints kubectl top and applies the proxy scrape.
si_preflight() {
  kubectl get nodes
  ! kubectl get nodes --no-headers | awk '$2 !~ /^Ready/' | grep . || { echo "a node is not Ready"; return 1; }
  command -v linkerd >/dev/null || { echo "install Linkerd and linkerd-viz first"; return 1; }
  if [ "${1:-}" = full ]; then
    kubectl top nodes
    linkerd check --proxy=false
    kubectl apply -f workloads/linkerd/podmonitor.yaml
  else
    linkerd check --proxy=false >/dev/null || { echo "linkerd check failed"; return 1; }
  fi
}

# si_series name: a series name that cannot run into another run id.
si_series() {
  [[ $1 =~ ^[a-z][a-z0-9]*$ ]] || { echo "series $1 is not lowercase letters and digits"; return 1; }
}

# si_scales v...: every value is a positive number.
si_scales() {
  for v in "$@"; do
    [[ $v =~ ^[0-9]*\.?[0-9]+$ ]] && awk "BEGIN{exit !($v > 0)}" || { echo "scale $v is not a positive number"; return 1; }
  done
}

# si_prepare_app app node [flag...]: a --mesh smoke run with its checks, then a one-pod capacity ramp.
si_prepare_app() {
  local app=$1 node=$2 ns; shift 2
  ns=$(python3 -m experiment.apps "$app" namespace)
  echo "== $app on $node: smoke run ($(date +%T))"
  uv run run.py web --app "$app" --node "$node" --prefix "$app-mesh-smoke" --profile constant --probes relaxed --mesh "$@"
  python3 -m experiment.interactions smoke "data/web/$app-mesh-smoke-constant"
  kubectl get hpa -n "$ns"
  ! linkerd viz check >/dev/null 2>&1 || linkerd viz edges deploy -n "$ns"
  echo "== $app on $node: one-pod ramp ($(date +%T))"
  uv run run.py web --app "$app" --node "$node" --prefix "$app-mesh-cap" --profile ramp --replicas 1 --probes relaxed --mesh "$@"
}

# si_run label app node prefix profile scale [flag...]: one series run, skipped when runs.jsonl already records it.
si_run() {
  local label=$1 app=$2 node=$3 prefix=$4 profile=$5 scale=$6; shift 6
  if [ -f data/web/runs.jsonl ] && grep -qF "\"run_id\": \"$prefix-$profile\"" data/web/runs.jsonl; then
    echo "[$label] $prefix-$profile recorded, skipping"; return 0
  fi
  echo "[$label] $prefix-$profile --scale $scale on $node ($(date +%T))"
  uv run run.py web --app "$app" --node "$node" --prefix "$prefix" --profile "$profile" --scale "$scale" --probes relaxed --mesh "$@" \
    || { echo "FAILED: $prefix-$profile. Fix the cause and rerun the same command to resume."; return 1; }
}
