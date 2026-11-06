# Roadmap

croft is experimental. These are directions, not dates.

## Now

- Validate the operand metrics (bytes, connect destinations, socket I/O) against the syscall counts, and measure their overhead.
- Finish the comparison of the HPA with `croft scale` alone, with published load settings.
- Analyse which services take a burst and which trigger scaling, across more workloads.

## Next

- Compare syscall patterns across service roles (frontend, API, database), and test whether they separate roles or only language runtimes.
- Package the collector for installation outside the lab, with a Helm chart and versioned images.

## Later

- Explore exposing the signals to existing autoscalers, such as a KEDA external scaler.
- Decode syscall numbers on architectures other than x86_64.
- Record file descriptor types with CO-RE.
